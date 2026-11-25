//! App chrome: design tokens, the top app bar, and the left navigation rail.
//!
//! Token values are the design system in `docs/ux/ux-design.html` — they are
//! deliberately separate from `shared::styles` (`mod.widgets.flex.*`), which
//! serves the legacy single-screen player.

use makepad_widgets::*;

use crate::api::Screen;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    use mod.text.*
    use mod.res.*

    // Namespace object for the UX design tokens.
    mod.widgets.ux = {}

    // ---------------------------------------------------------------- color --
    // The dorobot-ux palette, dark theme. These were a private copy that had
    // drifted: the surfaces matched, but every semantic colour was the *light*
    // variant painted on a dark ground — ACCENT was VIO_L, OK was OK_L, WARN
    // AMB_L, STOP HOT_L — which is why the orange read muted and the good/bad
    // chips looked dull. They are colours designed for a white background.
    //
    // Names are dorobot_ux::tokens::pal's, so a drift is greppable next time.
    mod.widgets.ux.GROUND      = #x1C1C1E   // pal::DEEP_D
    mod.widgets.ux.SURFACE     = #x2C2C2E   // pal::LIFT_D
    mod.widgets.ux.ELEVATED    = #x38383A   // pal::EDGE_D
    mod.widgets.ux.LINE        = #x38383A   // pal::EDGE_D
    mod.widgets.ux.INK         = #xF5F5F7   // pal::INK_D
    mod.widgets.ux.INK_2       = #xD1D1D6   // pal::INK2_D
    mod.widgets.ux.INK_3       = #xAEAEB2   // pal::DIM_D
    mod.widgets.ux.ACCENT      = #x0A84FF   // pal::VIO_D
    mod.widgets.ux.ACCENT_SOFT = #x19334D   // pal::VIOG_D — the accent's ground
    mod.widgets.ux.OK          = #x30D158   // pal::OK_D
    mod.widgets.ux.WARN        = #xFF9F0A   // pal::AMB_D
    mod.widgets.ux.STOP        = #xFF453A   // pal::HOT_D

    // ----------------------------------------------------------------- type --
    // PingFang follows the shared desktop sheet, with platform fallbacks.
    mod.widgets.ux.FONT_REGULAR = mod.widgets.nx.FONT_R{}
    mod.widgets.ux.FONT_MEDIUM = mod.widgets.nx.FONT_M{}
    mod.widgets.ux.FONT_BOLD = mod.widgets.nx.FONT_B{}

    // Lighter, smaller, airier than before — the previous scale read as heavy.
    mod.widgets.ux.TEXT_H1    = mod.widgets.ux.FONT_BOLD{font_size: 22.0}
    mod.widgets.ux.TEXT_TITLE = mod.widgets.ux.FONT_MEDIUM{font_size: 13.0}
    mod.widgets.ux.TEXT_BODY  = mod.widgets.ux.FONT_REGULAR{font_size: 11.5}
    mod.widgets.ux.TEXT_META  = mod.widgets.ux.FONT_REGULAR{font_size: 11.0}
    mod.widgets.ux.TEXT_CHIP  = mod.widgets.ux.FONT_MEDIUM{font_size: 10.0}
    mod.widgets.ux.TEXT_NAV   = mod.widgets.ux.FONT_MEDIUM{font_size: 12.0}

    // Shared toolbar treatment, including pressed, focused and selected states.
    mod.widgets.ux.ToolButton = mod.widgets.nx.NxBtn{
        height: 32 min_height: 32
        margin: 0.0
        align: Align{x: 0.5 y: 0.5}
        padding: Inset{left: 10. right: 10. top: 0. bottom: 0.}
        draw_text +: {text_style: mod.widgets.ux.TEXT_BODY{}}
        draw_bg +: {
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                let base = mix(#x242428, #xFFFFFF, self.light)
                let tint = mix(#x19334D, #xE5F0FF, self.light)
                let fill = mix(base, tint, max(self.on, self.hover * 0.5))
                let pressed = mix(#x34404E, #xD5E5F9, self.light)
                let edge = mix(#x48484A, #xD1D1D6, self.light)
                sdf.box(0.5, 0.5, self.rect_size.x-1.0, self.rect_size.y-1.0, 4.0)
                sdf.fill_keep(mix(fill, pressed, self.down))
                sdf.stroke(mix(edge, mix(#x64B5FF, #x0060CC, self.light), self.focus), 1.0)
                return sdf.result
            }
        }
    }

    mod.widgets.ux.QuietButton = mod.widgets.ux.ToolButton{
        draw_bg +: {
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                sdf.box(0.5, 0.5, self.rect_size.x-1.0, self.rect_size.y-1.0, 4.0)
                let tint = mix(#x303036, #xE9EDF3, self.light)
                sdf.fill_keep(mix(#x00000000, tint, max(self.hover, self.on)))
                sdf.stroke(mix(#x00000000, #x0A84FF, self.focus), 1.0)
                return sdf.result
            }
        }
    }
    mod.widgets.ux.SegmentButton = mod.widgets.ux.ToolButton{
        draw_bg +: {
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                sdf.box(0.5, 0.5, self.rect_size.x-1.0, self.rect_size.y-1.0, 4.0)
                let idle = mix(#x25252A, #xF2F3F5, self.light)
                let selected = mix(#x1D3855, #xE5F0FF, self.light)
                sdf.fill_keep(mix(idle, selected, max(self.on, self.hover * 0.4)))
                sdf.stroke(mix(#x00000000, #x0A84FF, self.focus), 1.0)
                return sdf.result
            }
        }
    }
    mod.widgets.ux.WorkspaceTab = mod.widgets.ux.QuietButton{
        width: Fit height: 38 min_height: 38
        padding: Inset{left: 16. right: 16. top: 0. bottom: 0.}
        draw_text +: {text_style: mod.widgets.ux.TEXT_NAV{}}
        draw_bg +: {
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                sdf.box(0.0, 0.0, self.rect_size.x, self.rect_size.y, 0.0)
                let base = mix(#x1C1C1E, #xF2F2F7, self.light)
                let selected = mix(#x20364E, #xE5F0FF, self.light)
                sdf.fill(mix(base, selected, max(self.on * 0.6, self.hover * 0.3)))
                sdf.box(10.0, self.rect_size.y - 2.0, self.rect_size.x - 20.0, 2.0, 1.0)
                sdf.fill(mix(#x00000000, mix(#x64B5FF, #x0060CC, self.light), self.on))
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 4.0)
                sdf.stroke(mix(#x00000000, #x0A84FF, self.focus), 1.0)
                return sdf.result
            }
        }
    }
    mod.widgets.ux.PrimaryButton = mod.widgets.ux.ToolButton{
        draw_bg +: {
            danger: instance(0.0)
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                sdf.box(0.5, 0.5, self.rect_size.x-1.0, self.rect_size.y-1.0, 4.0)
                let base = mix(#x0878EE, #xD72F3D, self.danger)
                let press = mix(#x0055B8, #xA31E2B, self.danger)
                let fill = mix(base, press, max(self.down, self.hover * 0.35))
                sdf.fill_keep(mix(fill, mix(#x333338, #xE5E5EA, self.light), self.disabled))
                sdf.stroke(mix(#x00000000, #xAFD6FF, self.focus), 1.5)
                return sdf.result
            }
        }
        draw_text +: {get_color: fn() {return mix(#xFFFFFF, mix(#x8E8E93, #x636366, self.light), self.disabled)}}
    }
    mod.widgets.ux.StopButton = mod.widgets.ux.PrimaryButton{draw_bg +: {danger: 1.0}}

    // -------------------------------------------------------------- surfaces --
    // Panel with a hairline border, the base for every card and rail.
    // `light` = 0 dark, 1 light. Every surface carries it so a theme switch is
    // one value pushed through the tree rather than a second widget set.
    mod.widgets.ux.Card = View{
        width: Fill
        height: Fit
        padding: 10.0
        show_bg: true
        draw_bg +: {
            light: instance(0.0)
            border_size: 1.0
            border_radius: 0.0
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                let fill = mix(#x1C1C1E, #xFFFFFF, self.light)
                let edge = mix(#x38383A, #xE5E5EA, self.light)
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, self.border_radius)
                sdf.fill_keep(fill)
                sdf.stroke(edge, 1.0)
                return sdf.result
            }
        }
    }

    // Standard panel title bar: drag dots, title, maximize, close.
    mod.widgets.ux.PanelHead = View{
        width: Fill
        height: 40
        flow: Right
        align: Align{y: 0.5}
        padding: Inset{left: 12. right: 10.}
        spacing: 9.0
        show_bg: false
        draw_bg +: {
            light: instance(0.0)
            pixel: fn() {
                // hairline along the bottom edge only
                let base = mix(#x2C2C2E, #xFFFFFF, self.light)
                let border = mix(#x38383A, #xE5E5EA, self.light)
                let t = step(self.rect_size.y - 1.0, self.pos.y * self.rect_size.y)
                return mix(base, border, t)
            }
        }

        grip := View{
            visible: false
            width: 10 height: 14
            show_bg: true
            draw_bg +: {
                light: instance(0.0)
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    let c = mix(#xAEAEB2, #x636366, self.light)
                    let r = 1.0
                    sdf.circle(2.0, 3.0, r)  sdf.fill(c)
                    sdf.circle(7.0, 3.0, r)  sdf.fill(c)
                    sdf.circle(2.0, 7.0, r)  sdf.fill(c)
                    sdf.circle(7.0, 7.0, r)  sdf.fill(c)
                    sdf.circle(2.0, 11.0, r) sdf.fill(c)
                    sdf.circle(7.0, 11.0, r) sdf.fill(c)
                    return sdf.result
                }
            }
        }
        title := Label{
            width: Fill max_lines: 1 text_overflow: Ellipsis
            text: "Panel"
            draw_text +: {
                light: instance(0.0)
                text_style: mod.widgets.ux.TEXT_TITLE{}
                get_color: fn() { return mix(#xD1D1D6, #x38383A, self.light) }
            }
        }
        btn_max := View{
            visible: false
            width: 13 height: 13
            show_bg: true
            draw_bg +: {
                light: instance(0.0)
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    sdf.box(2.0, 2.0, 9.0, 9.0, 1.5)
                    sdf.stroke(mix(#xAEAEB2, #x636366, self.light), 1.1)
                    return sdf.result
                }
            }
        }
        btn_close := View{
            visible: false
            width: 13 height: 13
            margin: Inset{left: 6.}
            show_bg: true
            draw_bg +: {
                light: instance(0.0)
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    sdf.move_to(3.0, 3.0)  sdf.line_to(10.0, 10.0)
                    sdf.stroke(mix(#xAEAEB2, #x636366, self.light), 1.1)
                    sdf.move_to(10.0, 3.0) sdf.line_to(3.0, 10.0)
                    sdf.stroke(mix(#xAEAEB2, #x636366, self.light), 1.1)
                    return sdf.result
                }
            }
        }
    }

    // ---------------------------------------------------------------- chips --
    // Status pill. `tone` selects the palette: 0 neutral, 1 ok, 2 stop, 3 accent.
    mod.widgets.ux.Chip = RoundedView{
        width: Fit
        // Intrinsic height: a fixed box clipped descenders at this type size.
        height: Fit
        padding: Inset{left: 8. right: 8. top: 4. bottom: 4.}
        align: Align{y: 0.5}
        show_bg: true
        draw_bg +: {
            tone: instance(0.0)
            light: instance(0.0)
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                // Shader `let` is immutable, so select branchlessly on tone.
                let f_ok = mix(#x38383A, #x38383A, step(0.5, self.tone))
                let f_stop = mix(f_ok, #x38383A, step(1.5, self.tone))
                let f_accent = mix(f_stop, #x38383A, step(2.5, self.tone))
                let fill = mix(f_accent, #x38383A, step(3.5, self.tone))
                let l_ok = mix(#x38383A, #x30D158, step(0.5, self.tone))
                let l_stop = mix(l_ok, #xFF453A, step(1.5, self.tone))
                let l_accent = mix(l_stop, #x0A84FF, step(2.5, self.tone))
                let line = mix(l_accent, #xFF9F0A, step(3.5, self.tone))
                // light palette: tinted washes instead of dark fills
                let lf_ok = mix(#xF2F2F7, #xF2F2F7, step(0.5, self.tone))
                let lf_stop = mix(lf_ok, #xF2F2F7, step(1.5, self.tone))
                let lf_accent = mix(lf_stop, #xF2F2F7, step(2.5, self.tone))
                let lfill = mix(lf_accent, #xF2F2F7, step(3.5, self.tone))
                let ll_ok = mix(#xE5E5EA, #xD1D1D6, step(0.5, self.tone))
                let ll_stop = mix(ll_ok, #xD70015, step(1.5, self.tone))
                let ll_accent = mix(ll_stop, #x007AFF, step(2.5, self.tone))
                let lline = mix(ll_accent, #x996300, step(3.5, self.tone))
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 14.0)
                sdf.fill_keep(mix(fill, lfill, self.light))
                sdf.stroke(mix(line, lline, self.light), 1.0)
                return sdf.result
            }
        }
        label := Label{
            text: "chip"
            draw_text +: {
                tone: instance(0.0)
                light: instance(0.0)
                text_style: mod.widgets.ux.TEXT_CHIP{}
                get_color: fn() {
                    let c_ok = mix(#xD1D1D6, #x30D158, step(0.5, self.tone))
                    let c_stop = mix(c_ok, #xFF6961, step(1.5, self.tone))
                    let c_accent = mix(c_stop, #x64B5FF, step(2.5, self.tone))
                    let dark_c = mix(c_accent, #xFFB340, step(3.5, self.tone))
                    let l_ok = mix(#x636366, #x1D7A34, step(0.5, self.tone))
                    let l_stop = mix(l_ok, #xD70015, step(1.5, self.tone))
                    let l_accent = mix(l_stop, #x0060CC, step(2.5, self.tone))
                    let light_c = mix(l_accent, #x996300, step(3.5, self.tone))
                    return mix(dark_c, light_c, self.light)
                }
            }
        }
    }

    // ------------------------------------------------------------- app bar --
    mod.widgets.ux.AppBar = View{
        width: Fill
        height: 58
        flow: Right
        align: Align{y: 0.5}
        padding: Inset{left: 18. right: 20.}
        spacing: 14.0
        show_bg: true
        draw_bg +: {
            light: instance(0.0)
            pixel: fn() {
                let base = mix(#x1C1C1E, #xFFFFFF, self.light)
                let border = mix(#x38383A, #xE5E5EA, self.light)
                let t = step(self.rect_size.y - 1.0, self.pos.y * self.rect_size.y)
                return mix(base, border, t)
            }
        }

        hamburger := View{
            width: 20 height: 20
            show_bg: true
            draw_bg +: {
                light: instance(0.0)
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    let c = mix(#xD1D1D6, #x636366, self.light)
                    sdf.move_to(2.0, 5.0)  sdf.line_to(18.0, 5.0)  sdf.stroke(c, 1.3)
                    sdf.move_to(2.0, 10.0) sdf.line_to(18.0, 10.0) sdf.stroke(c, 1.3)
                    sdf.move_to(2.0, 15.0) sdf.line_to(18.0, 15.0) sdf.stroke(c, 1.3)
                    return sdf.result
                }
            }
        }
        logo := View{
            width: 26 height: 26
            margin: Inset{left: 8.}
            show_bg: true
            draw_bg +: {
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    let c = #x007AFF
                    // stylised arm: base, upper link, forearm, gripper dot
                    sdf.box(3.0, 19.0, 13.0, 4.0, 1.2)
                    sdf.fill(c)
                    sdf.rotate(-0.6, 8.0, 19.0)
                    sdf.box(6.0, 9.0, 4.0, 11.0, 1.2)
                    sdf.fill(c)
                    sdf.rotate(0.6, 8.0, 19.0)
                    sdf.rotate(0.5, 12.0, 10.0)
                    sdf.box(10.0, 6.0, 11.0, 3.6, 1.2)
                    sdf.fill(c)
                    sdf.rotate(-0.5, 12.0, 10.0)
                    sdf.circle(20.0, 6.0, 2.6)
                    sdf.fill(#xD1D1D6)
                    return sdf.result
                }
            }
        }
        product := Label{
            text: "DoRobot Studio"
            draw_text +: {
                light: instance(0.0)
                text_style: mod.widgets.ux.FONT_MEDIUM{font_size: 14.0}
                get_color: fn() { return mix(#xE5E5EA, #x38383A, self.light) }
            }
        }
        Filler{}
        actions := View{
            width: Fit height: Fit
            flow: Right
            spacing: 18.0
            align: Align{y: 0.5}
            bell := View{
                width: 16 height: 16
                show_bg: true
                draw_bg +: {
                    light: instance(0.0)
                    pixel: fn() {
                        let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                        sdf.circle(8.5, 7.0, 5.4)
                        sdf.stroke(mix(#xD1D1D6, #x636366, self.light), 1.2)
                        sdf.move_to(5.5, 13.0) sdf.line_to(11.5, 13.0)
                        sdf.stroke(mix(#xD1D1D6, #x636366, self.light), 1.2)
                        return sdf.result
                    }
                }
            }
            help := View{
                width: 16 height: 16
                show_bg: true
                draw_bg +: {
                    light: instance(0.0)
                    pixel: fn() {
                        let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                        sdf.circle(8.5, 8.5, 7.2)
                        sdf.stroke(mix(#xD1D1D6, #x636366, self.light), 1.2)
                        sdf.circle(8.5, 12.0, 0.9)
                        sdf.fill(mix(#xD1D1D6, #x636366, self.light))
                        return sdf.result
                    }
                }
            }
            gear := View{
                width: 16 height: 16
                cursor: MouseCursor.Hand
                capture_overload: true
                show_bg: true
                draw_bg +: {
                    light: instance(0.0)
                    pixel: fn() {
                        let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                        sdf.circle(8.5, 8.5, 6.6)
                        sdf.stroke(mix(#xD1D1D6, #x636366, self.light), 1.2)
                        sdf.circle(8.5, 8.5, 2.4)
                        sdf.stroke(mix(#xD1D1D6, #x636366, self.light), 1.2)
                        return sdf.result
                    }
                }
            }
            avatar := RoundedView{
                width: 28 height: 28
                align: Align{x: 0.5 y: 0.5}
                show_bg: true
                draw_bg +: {
                    color: #x0A84FF
                    border_radius: 14.0
                }
                Label{
                    text: "AK"
                    draw_text +: {
                        color: #xFFFFFF
                        text_style: mod.widgets.ux.TEXT_CHIP{}
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------- nav rail --
    // One rail entry. Live uses a red pulse only while frames are arriving.
    mod.widgets.ux.NavItem = View{
        width: Fill
        height: 40
        flow: Right
        padding: Inset{left: 12.0 right: 12.0 top: 0.0 bottom: 0.0}
        align: Align{y: 0.5}
        spacing: 12.0
        cursor: MouseCursor.Hand
        show_bg: true
        draw_bg +: {
            active: instance(0.0)
            light: instance(0.0)
            streaming: instance(0.0)
            pulse: instance(0.0)
            pixel: fn() {
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                let well = mix(#x19334D, #xE5F0FF, self.light)
                let idle = mix(#x0F131B00, #xF7F8FB00, self.light)
                sdf.box(0.0, 0.0, self.rect_size.x, self.rect_size.y, 4.0)
                let live_red = mix(#x800F1A, #xD70015, self.pulse)
                sdf.fill(mix(mix(idle, well, self.active), live_red, self.streaming * self.active))
                return sdf.result
            }
        }

        glyph := View{
            // Any size works: the shader scales its 22-unit design space to
            // rect_size, so the artwork fits the button rather than being
            // clipped by it. It was drawn at fixed pixel coordinates reaching
            // x=20, so even the original 19px view cropped the folder's edge.
            width: 18 height: 18
            show_bg: true
            draw_bg +: {
                icon: instance(0.0)
                active: instance(0.0)
                light: instance(0.0)
                streaming: instance(0.0)
                pixel: fn() {
                    let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                    let idle_c = mix(#xD1D1D6, #x38383A, self.light)
                    let on_c = mix(#x64B5FF, #x0060CC, self.light)
                    let c = mix(mix(idle_c, on_c, self.active), mix(#xFF453A, #xFFFFFF, self.active), self.streaming)
                    // Design space is 22 units square; scale it to the view.
                    let s = self.rect_size.x / 22.0
                    let w1 = 1.5 * s
                    let w2 = 1.3 * s
                    if self.icon < 0.5 {
                        // library: folder
                        sdf.move_to(2.0 * s, 6.0 * s)
                        sdf.line_to(8.0 * s, 6.0 * s)
                        sdf.line_to(10.0 * s, 8.5 * s)
                        sdf.line_to(20.0 * s, 8.5 * s)
                        sdf.line_to(20.0 * s, 18.0 * s)
                        sdf.line_to(2.0 * s, 18.0 * s)
                        sdf.close_path()
                        sdf.stroke(c, w1)
                    } else if self.icon < 1.5 {
                        // record: ring with filled core
                        sdf.circle(11.0 * s, 11.0 * s, 8.0 * s)
                        sdf.stroke(c, w1)
                        sdf.circle(11.0 * s, 11.0 * s, 3.6 * s)
                        sdf.fill(c)
                    } else if self.icon < 2.5 {
                        // play: triangle
                        sdf.move_to(5.0 * s, 3.5 * s)
                        sdf.line_to(18.0 * s, 11.0 * s)
                        sdf.line_to(5.0 * s, 18.5 * s)
                        sdf.close_path()
                        sdf.stroke(c, w1)
                    } else if self.icon < 3.5 {
                        // hardware: chip
                        sdf.box(5.0 * s, 5.0 * s, 12.0 * s, 12.0 * s, 1.5 * s)
                        sdf.stroke(c, w1)
                        sdf.box(9.0 * s, 9.0 * s, 4.0 * s, 4.0 * s, 0.8 * s)
                        sdf.fill(c)
                        sdf.move_to(2.0 * s, 8.0 * s)  sdf.line_to(5.0 * s, 8.0 * s)  sdf.stroke(c, w2)
                        sdf.move_to(2.0 * s, 14.0 * s) sdf.line_to(5.0 * s, 14.0 * s) sdf.stroke(c, w2)
                        sdf.move_to(17.0 * s, 8.0 * s)  sdf.line_to(20.0 * s, 8.0 * s)  sdf.stroke(c, w2)
                        sdf.move_to(17.0 * s, 14.0 * s) sdf.line_to(20.0 * s, 14.0 * s) sdf.stroke(c, w2)
                    } else if self.icon < 4.5 {
                        // eval: bar chart
                        sdf.box(3.0 * s, 12.0 * s, 3.6 * s, 7.0 * s, 0.8 * s)  sdf.fill(c)
                        sdf.box(9.0 * s, 7.0 * s, 3.6 * s, 12.0 * s, 0.8 * s)  sdf.fill(c)
                        sdf.box(15.0 * s, 3.0 * s, 3.6 * s, 16.0 * s, 0.8 * s) sdf.fill(c)
                    } else {
                        // live: broadcast rings
                        sdf.circle(11.0 * s, 11.0 * s, 8.5 * s) sdf.stroke(c, w2)
                        sdf.circle(11.0 * s, 11.0 * s, 5.0 * s) sdf.stroke(c, w2)
                        sdf.circle(11.0 * s, 11.0 * s, 1.8 * s) sdf.fill(c)
                    }
                    return sdf.result
                }
            }
        }
        caption := Label{
            // Center in the whole navigation row. The default Label padding
            // overflowed the old 24px content height and pushed its ink down.
            width: Fill height: Fit
            padding: 0.0 margin: 0.0
            align: Align{x: 0.0 y: 0.5}
            text: "Item"
            draw_text +: {
                ink_centered: true
                temp_y_shift: #(if cfg!(target_os = "macos") { -0.10f64 } else { 0.0f64 })
                active: instance(0.0)
                light: instance(0.0)
                streaming: instance(0.0)
                text_style: mod.widgets.ux.TEXT_NAV{}
                get_color: fn() {
                    let idle_c = mix(#xD1D1D6, #x38383A, self.light)
                    let on_c = mix(#x64B5FF, #x0060CC, self.light)
                    return mix(mix(idle_c, on_c, self.active), mix(#xFF453A, #xFFFFFF, self.active), self.streaming)
                }
            }
        }
    }

    mod.widgets.ux.NavRail = View{
        width: 172
        height: Fill
        flow: Down
        padding: 0.0
        spacing: 4.0
        show_bg: false
        draw_bg +: {
            light: instance(0.0)
            pixel: fn() {
                let base = mix(#x1C1C1E, #xFFFFFF, self.light)
                let border = mix(#x38383A, #xE5E5EA, self.light)
                let t = step(self.rect_size.x - 1.0, self.pos.x * self.rect_size.x)
                return mix(base, border, t)
            }
        }

        nav_hardware := mod.widgets.ux.NavItem{ caption +: { text: "Robot setup" } glyph +: { draw_bg +: {icon: 3.0} } }
        nav_live     := mod.widgets.ux.NavItem{ caption +: { text: "Operate" } glyph +: { draw_bg +: {icon: 5.0} } }
        nav_record   := mod.widgets.ux.NavItem{ caption +: { text: "Record" } glyph +: { draw_bg +: {icon: 1.0} } }
        nav_play     := mod.widgets.ux.NavItem{ caption +: { text: "Replay" } glyph +: { draw_bg +: {icon: 2.0} } }
        View{height: 16 width: Fill}
        nav_library  := mod.widgets.ux.NavItem{ caption +: { text: "Library" } glyph +: { draw_bg +: {icon: 0.0} } }
        nav_eval     := mod.widgets.ux.NavItem{ caption +: { text: "Evaluate" } glyph +: { draw_bg +: {icon: 4.0} } }
    }
}

/// Nav rail ids in display order, paired with the screen each selects.
pub const NAV_ITEMS: [(&[LiveId], Screen); 6] = [
    (ids!(nav_hardware), Screen::Hardware),
    (ids!(nav_live), Screen::Live),
    (ids!(nav_record), Screen::Record),
    (ids!(nav_play), Screen::Play),
    (ids!(nav_library), Screen::Library),
    (ids!(nav_eval), Screen::Eval),
];

/// Paint the active pill and label tint for the current screen.
pub fn sync_nav(cx: &mut Cx, rail: &WidgetRef, current: Screen) {
    for (path, screen) in NAV_ITEMS {
        let on = if screen == current { 1.0 } else { 0.0 };
        let mut item = rail.widget(cx, path);
        if item.is_empty() {
            continue;
        }
        script_apply_eval!(cx, item, {
            draw_bg +: { active: #(on) }
        });
        let mut glyph = rail.widget(cx, path).widget(cx, ids!(glyph));
        if !glyph.is_empty() {
            script_apply_eval!(cx, glyph, {
                draw_bg +: { active: #(on) }
            });
        }
        let mut cap = rail.widget(cx, path).widget(cx, ids!(caption));
        if !cap.is_empty() {
            script_apply_eval!(cx, cap, {
                draw_text +: { active: #(on) }
            });
        }
    }
}

/// 插件分组与顶部标签共用的选中及主题状态。
pub fn sync_nav_button(cx: &mut Cx, button: &WidgetRef, selected: bool, light: f64) {
    if button.is_empty() { return; }
    let on = if selected { 1.0 } else { 0.0 };
    let mut button = button.clone();
    script_apply_eval!(cx, button, {
        draw_bg +: {on: #(on) light: #(light)}
        draw_text +: {on: #(on) light: #(light)}
    });
}

// ============================================================================
// Theme
// ============================================================================

use std::cell::Cell;

thread_local! {
    /// 0.0 = dark, 1.0 = light. Read by screens when they repaint.
    static LIGHT: Cell<f64> = const { Cell::new(0.0) };
}

pub fn light_mode() -> f64 {
    LIGHT.with(|l| l.get())
}

pub fn set_light_mode(v: f64) {
    LIGHT.with(|l| l.set(v));
}

pub fn toggle_light_mode() -> f64 {
    let next = if light_mode() > 0.5 { 0.0 } else { 1.0 };
    set_light_mode(next);
    next
}

/// True when a View-like widget was released under the pointer.
///
/// `find_widget_action` returns only the *first* action for a uid, and one
/// press delivers FingerDown and FingerUp in the same batch — so matching on
/// that first action never sees the release. Scan them all instead.
pub fn view_clicked(actions: &Actions, uid: WidgetUid) -> bool {
    actions
        .filter_widget_actions(uid)
        .any(|a| matches!(a.cast::<ViewAction>(), ViewAction::FingerUp(fe) if fe.is_over))
}

/// Which draws on a widget carry the `light` instance.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Themed {
    Bg,
    Text,
    Both,
}

/// Push the theme value into a set of widgets.
///
/// Widgets are listed explicitly rather than walked: applying a field a widget
/// does not declare logs a script-VM error, and composite widgets have no
/// `draw_bg` of their own.
pub fn apply_light(cx: &mut Cx, root: &WidgetRef, items: &[(&[LiveId], Themed)], light: f64) {
    for (path, kind) in items {
        let mut w = root.widget(cx, path);
        if w.is_empty() {
            continue;
        }
        match kind {
            Themed::Bg => script_apply_eval!(cx, w, { draw_bg +: { light: #(light) } }),
            Themed::Text => script_apply_eval!(cx, w, { draw_text +: { light: #(light) } }),
            Themed::Both => script_apply_eval!(cx, w, {
                draw_bg +: { light: #(light) }
                draw_text +: { light: #(light) }
            }),
        }
    }
}

/// Same as [`apply_light`] but rooted at a `View` (what a widget's `#[deref]`
/// field is), rather than a `WidgetRef`.
pub fn apply_light_in(cx: &mut Cx, root: &mut View, items: &[(&[LiveId], Themed)], light: f64) {
    for (path, kind) in items {
        let mut w = root.widget(cx, path);
        if w.is_empty() {
            continue;
        }
        match kind {
            Themed::Bg => script_apply_eval!(cx, w, { draw_bg +: { light: #(light) } }),
            Themed::Text => script_apply_eval!(cx, w, { draw_text +: { light: #(light) } }),
            Themed::Both => script_apply_eval!(cx, w, {
                draw_bg +: { light: #(light) }
                draw_text +: { light: #(light) }
            }),
        }
    }
}

/// App-bar and nav-rail chrome.
pub fn theme_chrome(cx: &mut Cx, ui: &WidgetRef, light: f64) {
    let mut items: Vec<(&[LiveId], Themed)> = vec![
        (ids!(app_bar), Themed::Bg),
        (ids!(app_bar.hamburger), Themed::Bg),
        (ids!(app_bar.product), Themed::Text),
        (ids!(app_bar.actions.bell), Themed::Bg),
        (ids!(app_bar.actions.help), Themed::Bg),
        (ids!(app_bar.actions.gear), Themed::Bg),
        (ids!(nav), Themed::Bg),
    ];
    for (path, _) in NAV_ITEMS {
        items.push((path, Themed::Bg));
    }
    apply_light(cx, ui, &items, light);
    for (path, _) in NAV_ITEMS {
        let item = ui.widget(cx, path);
        if item.is_empty() {
            continue;
        }
        apply_light(
            cx,
            &item,
            &[(ids!(glyph), Themed::Bg), (ids!(caption), Themed::Text)],
            light,
        );
    }
}

/// Every panel head shares the same themed parts.
pub fn theme_panel_head(cx: &mut Cx, head: &WidgetRef, light: f64) {
    let mut h = head.clone();
    if !h.is_empty() {
        script_apply_eval!(cx, h, { draw_bg +: { light: #(light) } });
    }
    apply_light(
        cx,
        head,
        &[
            (ids!(grip), Themed::Bg),
            (ids!(title), Themed::Text),
            (ids!(btn_max), Themed::Bg),
            (ids!(btn_close), Themed::Bg),
        ],
        light,
    );
}

/// A chip carries the value on both its fill and its label.
pub fn theme_chip(cx: &mut Cx, chip: &WidgetRef, light: f64) {
    let mut c = chip.clone();
    if !c.is_empty() {
        script_apply_eval!(cx, c, { draw_bg +: { light: #(light) } });
    }
    apply_light(cx, chip, &[(ids!(label), Themed::Text)], light);
}
