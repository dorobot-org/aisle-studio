"""Run the real Dora guard with invalid traffic; no simulator/hardware needed.

Opt in with NANO_DORA_GRAPH_TESTS=1 and Dora 1.0.1 in PATH (or DORA_BIN).
"""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parent


@unittest.skipUnless(
    os.environ.get("NANO_DORA_GRAPH_TESTS") == "1", "explicit Dora runtime test"
)
class DoraGuardGraphTests(unittest.TestCase):
    def test_invalid_commands_never_reach_guard_output(self):
        cli = os.environ.get("DORA_BIN") or shutil.which("dora")
        self.assertTrue(cli, "Dora CLI is required")
        for fault in ("nan", "stale", "limit", "velocity"):
            with (
                self.subTest(fault=fault),
                tempfile.TemporaryDirectory(prefix="nano-dora-guard-") as directory,
            ):
                root = Path(directory)
                config = dict(
                    model=str(ROOT.parents[1] / "data/adora_nano/adora_nano.urdf"),
                    frames=100,
                    output=str(root),
                )
                (root / "config.json").write_text(json.dumps(config))
                (root / "probe.py").write_text("""import json,sys
from pathlib import Path
import pyarrow as pa
from dora import Node
from nano_dora import request_metadata
from nano_validation import load_model
root=Path(sys.argv[1]); fault=sys.argv[2]; model=load_model(Path(sys.argv[3])); node=Node()
node.send_output('command',pa.array([0.]*17,type=pa.float32()),request_metadata(0,100,model))
received=[]
for event in node:
 if event['type'] in ('STOP','ERROR'): break
 if event['type']!='INPUT': continue
 received.append(event['metadata']['request_id'])
 (root/'received.json').write_text(json.dumps(received))
 if len(received)>1: raise RuntimeError('Invalid command passed guard')
 values=[0.]*17; meta=request_metadata(0 if fault=='stale' else 1,100,model)
 index=model['names'].index('left_j1_yaw')
 values[index]={'nan':float('nan'),'stale':0.,'limit':9.,'velocity':1.}[fault]
 node.send_output('command',pa.array(values,type=pa.float32()),meta)
""")
                env = dict(
                    PYTHONPATH=str(ROOT), NANO_DORA_CONFIG=str(root / "config.json")
                )
                graph = {
                    "nodes": [
                        dict(
                            id="probe",
                            path=sys.executable,
                            args=f"-u {root}/probe.py {root} {fault} {config['model']}",
                            env=env,
                            inputs={"safe": "guard/joint_cmd_safe"},
                            outputs=["command"],
                        ),
                        dict(
                            id="guard",
                            path=sys.executable,
                            args=f"-u {ROOT}/nano_dora_node.py limits",
                            env=env,
                            inputs={"command": "probe/command"},
                            outputs=["joint_cmd_safe"],
                        ),
                    ]
                }
                (root / "graph.yaml").write_text(yaml.safe_dump(graph))
                run = subprocess.run(
                    [cli, "run", str(root / "graph.yaml"), "--stop-after", "15s"],
                    cwd=root,
                    capture_output=True,
                    text=True,
                    timeout=25,
                )
                self.assertNotEqual(run.returncode, 0, run.stdout[-2000:])
                self.assertEqual(json.loads((root / "received.json").read_text()), [0])
                rejection = json.loads((root / "guard-rejection.json").read_text())
                self.assertFalse(rejection["forwarded"])


if __name__ == "__main__":
    unittest.main()
