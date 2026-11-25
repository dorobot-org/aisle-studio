import json
import math
import os
import pty
import select
import threading
import time
import unittest
from feetech import BaseMapper, Bus, Guard, IDS, LIMIT, MockSerial, MotorWorker, REG, TIMEOUT_NS, mix, packet, speed_word


def command(seq=0, now=1_000_000_000, enabled=False, held=False, neutral=True, wheels=None):
    return dict(sequence=seq, input_ns=now, enabled=enabled, enable_held=held, neutral=neutral,
                wheels=wheels or [0., 0., 0.], connected=True, status="test")


class ProtocolTests(unittest.TestCase):
    def test_known_packets_and_signed_words(self):
        self.assertEqual(packet(13, 3, [46, 0x2c, 0x81]), bytes.fromhex("ff ff 0d 05 03 2e 2c 81 0f"))
        self.assertEqual(speed_word(-300), 0x812c)
        self.assertEqual(speed_word(300), 300)
        for value in (math.nan, math.inf, -1501, 1501):
            with self.assertRaises(ValueError):
                speed_word(value)

    def test_supplied_driver_mix_and_uniform_saturation(self):
        self.assertEqual(mix(100, 0, 0), [-86.6, 0, 86.6])
        self.assertEqual(mix(0, 100, 0), [50, -100, 50])
        self.assertEqual(mix(0, 0, 100), [100, 100, 100])
        wheels = mix(300, 300, 300)
        raw = [-.866*300+150+300, 0, .866*300+150+300]
        for a, b in zip(wheels, raw):
            self.assertAlmostEqual(a, b/max(raw)*LIMIT)

    def test_initialization_never_enables_torque(self):
        serial = MockSerial()
        bus = Bus(serial)
        bus.prepare()
        self.assertEqual(len(serial.writes), 15)
        self.assertFalse(any(p[5:7] == bytes([REG["torque"], 1]) for p in serial.writes))
        self.assertTrue(all(serial.registers[motor, REG["speed"]] == 0 for motor in IDS))

    def test_enable_and_release_write_all_motors(self):
        serial = MockSerial()
        bus = Bus(serial)
        bus.prepare()
        bus.drive([-100, 0, 100], True)
        self.assertEqual([serial.registers[m, REG["torque"]] for m in IDS], [1, 1, 1])
        self.assertEqual([serial.registers[m, REG["speed"]] for m in IDS], [0x8064, 0, 100])
        bus.drive([200]*3, False)
        self.assertEqual([serial.registers[m, REG["speed"]] for m in IDS], [0, 0, 0])
        self.assertEqual([serial.registers[m, REG["torque"]] for m in IDS], [0, 0, 0])

    def test_ack_failures_are_not_reported_as_success(self):
        for fault in ("checksum", "wrong_id", "motor_error", "timeout"):
            with self.subTest(fault=fault):
                serial = MockSerial()
                serial.fault = fault
                bus = Bus(serial)
                with self.assertRaises(OSError):
                    bus.write(13, "torque", 0)
                errors = bus.stop()
                self.assertEqual(len(errors), 6)
                self.assertEqual({p[2] for p in serial.writes}, set(IDS))

    @unittest.skipUnless(os.name == "posix", "PTY requires POSIX")
    def test_real_serial_transport_over_pty(self):
        import serial
        master, slave = pty.openpty()
        finished = threading.Event()
        writes = []
        def emulate():
            buffer = bytearray()
            while not finished.is_set():
                if not select.select([master], [], [], .05)[0]:
                    continue
                try:
                    buffer.extend(os.read(master, 1024))
                except OSError:
                    break
                while len(buffer) >= 4 and len(buffer) >= buffer[3]+4:
                    frame = bytes(buffer[:buffer[3]+4])
                    del buffer[:len(frame)]
                    writes.append(frame)
                    # Fragment responses to exercise partial serial reads.
                    ack = packet(frame[2], 0)
                    os.write(master, ack[:3])
                    time.sleep(.001)
                    os.write(master, ack[3:])
        thread = threading.Thread(target=emulate)
        transport = serial.Serial(os.ttyname(slave), 9600, timeout=.02, write_timeout=.02, exclusive=True)
        thread.start()
        try:
            bus = Bus(transport)
            bus.prepare()
            bus.drive([-100, 150, -50], True)
            self.assertEqual(bus.stop(), [])
            self.assertEqual(bus.acks, len(writes))
            self.assertEqual(len(writes), 30)
        finally:
            finished.set()
            thread.join(timeout=1)
            transport.close()
            os.close(master)
            os.close(slave)


class ControlTests(unittest.TestCase):
    def test_deadman_requires_release_neutral_after_connect(self):
        mapper = BaseMapper()
        sample = dict(sequence=0, input_ns=1, connected=True, axes={"left_y": -1}, buttons={"enable": True})
        self.assertFalse(mapper.command(sample)["enabled"])
        sample.update(axes={}, buttons={})
        self.assertFalse(mapper.command(sample)["enabled"])
        sample.update(axes={"left_y": -1}, buttons={"enable": True})
        self.assertTrue(mapper.command(sample)["enabled"])
        sample["connected"] = False
        self.assertFalse(mapper.command(sample)["enabled"])
        sample["connected"] = True
        self.assertFalse(mapper.command(sample)["enabled"])

    def test_pause_and_strafe(self):
        mapper = BaseMapper()
        s = dict(sequence=0, input_ns=1, connected=True, axes={}, buttons={})
        mapper.command(s)
        s.update(axes={"left_x": 1}, buttons={"enable": True})
        self.assertEqual(mapper.command(s)["wheels"], [-150, 300, -150])
        s["buttons"]["pause"] = True
        self.assertFalse(mapper.command(s)["enabled"])

    def test_guard_rejects_invalid_order_values_and_dimensions(self):
        guard = Guard()
        guard.accept(command(), 1_000_000_000)
        with self.assertRaises(ValueError):
            guard.accept(command(), 1_000_000_000)
        for wheels in ([math.nan, 0, 0], [301, 0, 0], [0, 0]):
            with self.assertRaises(ValueError):
                Guard().accept(command(wheels=wheels), 1_000_000_000)

    def test_guard_expiry_future_and_release_are_immediate(self):
        for age in (-1, TIMEOUT_NS+1):
            result = Guard().accept(command(enabled=True, held=True, wheels=[200]*3), 1_000_000_000+age)
            self.assertEqual(result["wheels"], [0, 0, 0])
        guard = Guard()
        result = guard.accept(command(enabled=True, held=True, wheels=[200]*3), 1_000_000_000)
        self.assertEqual(result["wheels"], [12, 12, 12])
        result = guard.accept(command(seq=1, enabled=False), 1_020_000_000)
        self.assertEqual(result["wheels"], [0, 0, 0])

    def test_driver_timeout_latches_until_release_and_neutral(self):
        bus = Bus(MockSerial())
        worker = MotorWorker(bus, lambda _: None)
        now = 1_000_000_000
        worker.submit(command(now=now))
        worker.step(now)
        worker.submit(command(seq=1, now=now, enabled=True, held=True, neutral=False, wheels=[100]*3))
        self.assertTrue(worker.step(now)["enabled"])
        self.assertFalse(worker.step(now+TIMEOUT_NS+1)["enabled"])
        worker.submit(command(seq=2, now=now+TIMEOUT_NS+2, enabled=True, held=True, neutral=False, wheels=[100]*3))
        self.assertFalse(worker.step(now+TIMEOUT_NS+2)["enabled"])
        worker.submit(command(seq=3, now=now+TIMEOUT_NS+3))
        worker.step(now+TIMEOUT_NS+3)
        worker.submit(command(seq=4, now=now+TIMEOUT_NS+4, enabled=True, held=True, wheels=[100]*3))
        self.assertTrue(worker.step(now+TIMEOUT_NS+4)["enabled"])

    def test_watchdog_stops_with_no_more_dora_events(self):
        states = []
        bus = Bus(MockSerial())
        worker = MotorWorker(bus, states.append, True)
        worker.submit(command(now=time.monotonic_ns()))
        worker.thread.start()
        try:
            time.sleep(.04)
            worker.submit(command(seq=1, now=time.monotonic_ns(), enabled=True, held=True, wheels=[100]*3))
            time.sleep(.04)
            self.assertTrue(states[-1]["enabled"])
            time.sleep(.3)
            self.assertFalse(states[-1]["enabled"])
            self.assertEqual(states[-1]["command_ticks"], [0, 0, 0])
        finally:
            worker.close()
        self.assertEqual(states[-1]["phase"], "stopped")
        self.assertTrue(states[-1]["stop_confirmed"])

    def test_lost_supervisor_stops_independently_of_dora(self):
        alive = threading.Event()
        alive.set()
        states = []
        worker = MotorWorker(Bus(MockSerial()), states.append, True, alive.is_set)
        worker.submit(command(now=time.monotonic_ns()))
        worker.thread.start()
        time.sleep(.04)
        worker.submit(command(seq=1, now=time.monotonic_ns(), enabled=True, held=True, wheels=[100]*3))
        time.sleep(.04)
        self.assertTrue(states[-1]["enabled"])
        alive.clear()
        worker.thread.join(timeout=1)
        self.assertFalse(worker.thread.is_alive())
        self.assertEqual(states[-1]["phase"], "fault")
        self.assertTrue(states[-1]["stop_confirmed"])
        self.assertEqual(states[-1]["command_ticks"], [0, 0, 0])


if __name__ == "__main__":
    unittest.main()
