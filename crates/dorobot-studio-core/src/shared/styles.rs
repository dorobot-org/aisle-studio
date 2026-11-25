//! LeRobot Viewer Styles and Theme
//!
//! Supports dark/light theme via `dark_mode: instance(0.0)` shader variable.
//! Use `get_global_dark_mode()` from makepad-app-shell to get current theme.

use makepad_widgets::*;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    use mod.text.*
    use mod.res.*

    // Namespace object for this app's shared style constants
    mod.widgets.flex = {}

    // ===========================================
    // FONTS - framework iOS typography with Unicode fallbacks
    // ===========================================

    mod.widgets.flex.FONT_REGULAR = mod.widgets.nx.FONT_R{}
    mod.widgets.flex.FONT_MEDIUM = mod.widgets.nx.FONT_M{}
    mod.widgets.flex.FONT_SEMIBOLD = mod.widgets.nx.FONT_B{}
    mod.widgets.flex.FONT_BOLD = mod.widgets.nx.FONT_B{}

    // ===========================================
    // TEXT STYLES (shared framework typography)
    // ===========================================

    mod.widgets.flex.TEXT_TITLE = mod.widgets.flex.FONT_BOLD{
        font_size: 18.0
    }

    mod.widgets.flex.TEXT_SUBTITLE = mod.widgets.flex.FONT_SEMIBOLD{
        font_size: 14.0
    }

    mod.widgets.flex.TEXT_PANEL_TITLE = mod.widgets.flex.FONT_SEMIBOLD{
        font_size: 11.0
    }

    mod.widgets.flex.TEXT_BODY = mod.widgets.flex.FONT_REGULAR{
        font_size: 12.0
        line_spacing: 1.4
    }

    mod.widgets.flex.TEXT_SMALL = mod.widgets.flex.FONT_REGULAR{
        font_size: 10.0
    }

    mod.widgets.flex.TEXT_MONO = mod.widgets.nx.FONT_MONO{
        font_size: 11.0
    }

    mod.widgets.flex.TEXT_BUTTON = mod.widgets.flex.FONT_SEMIBOLD{
        font_size: 12.0
    }

    // ===========================================
    // THEMED BACKGROUNDS (dark_mode responsive)
    // ===========================================

    // Sidebar background - responds to dark_mode
    mod.widgets.flex.ThemedSidebarBg = {
        dark_mode: instance(0.0)
        pixel: fn() {
            // Light: slate-50, Dark: #151518
            let light = vec4(1.000, 1.000, 1.000, 1.0)
            let dark = vec4(0.110, 0.110, 0.118, 1.0)
            return mix(light, dark, self.dark_mode)
        }
    }

    // Header background - responds to dark_mode
    mod.widgets.flex.ThemedHeaderBg = {
        dark_mode: instance(0.0)
        pixel: fn() {
            // Light: slate-100, Dark: #222228
            let light = vec4(0.949, 0.949, 0.969, 1.0)
            let dark = vec4(0.220, 0.220, 0.227, 1.0)
            return mix(light, dark, self.dark_mode)
        }
    }

    // Panel background - responds to dark_mode
    mod.widgets.flex.ThemedPanelBg = {
        dark_mode: instance(0.0)
        pixel: fn() {
            // Light: white, Dark: #1a1a1f
            let light = vec4(1.000, 1.000, 1.000, 1.0)
            let dark = vec4(0.110, 0.110, 0.118, 1.0)
            return mix(light, dark, self.dark_mode)
        }
    }

    // Divider - responds to dark_mode
    mod.widgets.flex.ThemedDividerBg = {
        dark_mode: instance(0.0)
        pixel: fn() {
            // Light: slate-200, Dark: #333340
            let light = vec4(0.898, 0.898, 0.918, 1.0)
            let dark = vec4(0.220, 0.220, 0.227, 1.0)
            return mix(light, dark, self.dark_mode)
        }
    }

    // Button background - responds to dark_mode
    // (applied to Button draw_bg; the base hover/down instances drive the states)
    mod.widgets.flex.ThemedButtonBg = {
        dark_mode: instance(0.0)
        pixel: fn() {
            // Light mode colors
            let light_normal = vec4(0.949, 0.949, 0.969, 1.0)  // slate-100
            let light_hover = vec4(0.898, 0.898, 0.918, 1.0)   // slate-200
            let light_pressed = vec4(0.820, 0.820, 0.839, 1.0) // slate-300
            // Dark mode colors
            let dark_normal = vec4(0.220, 0.220, 0.227, 1.0)   // #2a2a32
            let dark_hover = vec4(0.220, 0.220, 0.227, 1.0)    // #2d2d35
            let dark_pressed = vec4(0.220, 0.220, 0.227, 1.0)  // slightly lighter

            let normal = mix(light_normal, dark_normal, self.dark_mode)
            let hover_color = mix(light_hover, dark_hover, self.dark_mode)
            let pressed_color = mix(light_pressed, dark_pressed, self.dark_mode)

            let color = mix(normal, hover_color, self.hover)
            let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 10.0)
                        sdf.fill(mix(color, pressed_color, self.down))
                        return sdf.result
        }
    }

    // Accent button (primary action)
    mod.widgets.flex.ThemedAccentButtonBg = {
        pixel: fn() {
            // Accent color stays consistent
            let normal = vec4(0.000, 0.478, 1.000, 1.0)    // #4c8bf5
            let hover_color = vec4(0.000, 0.478, 1.000, 1.0) // #6ba1ff
            let pressed_color = vec4(0.000, 0.478, 1.000, 1.0)

            let color = mix(normal, hover_color, self.hover)
            let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 10.0)
                        sdf.fill(mix(color, pressed_color, self.down))
                        return sdf.result
        }
    }

    // ===========================================
    // THEMED TEXT DRAW SHADERS
    // ===========================================

    // Primary text - dark in light mode, light in dark mode
    mod.widgets.flex.ThemedTextPrimary = {
        dark_mode: instance(0.0)
        get_color: fn() {
            // Light mode: dark gray text, Dark mode: light gray text
            let light_text = vec4(0.110, 0.110, 0.118, 1.0)   // near black
            let dark_text = vec4(0.898, 0.898, 0.918, 1.0) // #e0e0e0
            return mix(light_text, dark_text, self.dark_mode)
        }
    }

    // Secondary text - medium gray in both modes
    mod.widgets.flex.ThemedTextSecondary = {
        dark_mode: instance(0.0)
        get_color: fn() {
            // Light mode: medium dark gray, Dark mode: medium light gray
            let light_text = vec4(0.388, 0.388, 0.400, 1.0)   // dark gray
            let dark_text = vec4(0.557, 0.557, 0.576, 1.0) // #888890
            return mix(light_text, dark_text, self.dark_mode)
        }
    }

    // Muted text - lighter labels/headers
    mod.widgets.flex.ThemedTextMuted = {
        dark_mode: instance(0.0)
        get_color: fn() {
            // Light mode: medium gray, Dark mode: dim gray
            let light_text = vec4(0.557, 0.557, 0.576, 1.0)   // gray
            let dark_text = vec4(0.388, 0.388, 0.400, 1.0) // #555560
            return mix(light_text, dark_text, self.dark_mode)
        }
    }

    // ===========================================
    // STATIC COLORS (for reference, non-themed)
    // ===========================================

    mod.widgets.flex.COLOR_ACCENT = #x007AFF
    mod.widgets.flex.COLOR_ACCENT_HOVER = #x007AFF
    mod.widgets.flex.COLOR_SUCCESS = #x248A3D
    mod.widgets.flex.COLOR_WARNING = #x996300
    mod.widgets.flex.COLOR_ERROR = #xD70015
    mod.widgets.flex.COLOR_PLAYHEAD = #xD70015

    // Waveform channel colors (consistent across themes)
    mod.widgets.flex.COLOR_CHANNEL_0 = #x007AFF
    mod.widgets.flex.COLOR_CHANNEL_1 = #x248A3D
    mod.widgets.flex.COLOR_CHANNEL_2 = #x996300
    mod.widgets.flex.COLOR_CHANNEL_3 = #xD70015
    mod.widgets.flex.COLOR_CHANNEL_4 = #x0A84FF
    mod.widgets.flex.COLOR_CHANNEL_5 = #x007EAD
    mod.widgets.flex.COLOR_CHANNEL_6 = #x248A3D

    // ===========================================
    // THEMED WIDGET TEMPLATES
    // ===========================================

    // Themed sidebar View
    mod.widgets.flex.ThemedSidebar = View{
        width: Fill
        height: Fill
        flow: Down
        show_bg: true
        draw_bg +: {
            dark_mode: instance(0.0)
            pixel: fn() {
                // Light: slate-50, Dark: #151518
                let light = vec4(1.000, 1.000, 1.000, 1.0)
                let dark = vec4(0.110, 0.110, 0.118, 1.0)
                return mix(light, dark, self.dark_mode)
            }
        }
    }

    // Themed header View
    mod.widgets.flex.ThemedHeader = View{
        width: Fill
        height: 40
        padding: Inset{left: 16.}
        align: Align{y: 0.5}
        show_bg: true
        draw_bg +: {
            dark_mode: instance(0.0)
            pixel: fn() {
                // Light: slate-100, Dark: #222228
                let light = vec4(0.949, 0.949, 0.969, 1.0)
                let dark = vec4(0.220, 0.220, 0.227, 1.0)
                return mix(light, dark, self.dark_mode)
            }
        }
    }

    // Themed panel container
    mod.widgets.flex.ThemedPanel = View{
        show_bg: true
        draw_bg +: {
            dark_mode: instance(0.0)
            pixel: fn() {
                // Light: white, Dark: #1a1a1f
                let light = vec4(1.000, 1.000, 1.000, 1.0)
                let dark = vec4(0.110, 0.110, 0.118, 1.0)
                return mix(light, dark, self.dark_mode)
            }
        }
    }

    // Themed divider
    mod.widgets.flex.ThemedDivider = View{
        width: Fill
        height: 1
        show_bg: true
        draw_bg +: {
            dark_mode: instance(0.0)
            pixel: fn() {
                // Light: slate-200, Dark: #333340
                let light = vec4(0.898, 0.898, 0.918, 1.0)
                let dark = vec4(0.220, 0.220, 0.227, 1.0)
                return mix(light, dark, self.dark_mode)
            }
        }
    }

    // Themed label with primary text color (dark in light mode, light in dark mode)
    mod.widgets.flex.ThemedLabel = Label{
        draw_text +: {
            text_style: mod.widgets.flex.FONT_SEMIBOLD{font_size: 12.0}
            dark_mode: instance(0.0)
            get_color: fn() {
                let light_text = vec4(0.110, 0.110, 0.118, 1.0)
                let dark_text = vec4(0.898, 0.898, 0.918, 1.0)
                return mix(light_text, dark_text, self.dark_mode)
            }
        }
    }

    // Themed secondary label
    mod.widgets.flex.ThemedLabelSecondary = Label{
        draw_text +: {
            text_style: mod.widgets.flex.FONT_REGULAR{font_size: 11.0}
            dark_mode: instance(0.0)
            get_color: fn() {
                let light_text = vec4(0.388, 0.388, 0.400, 1.0)
                let dark_text = vec4(0.557, 0.557, 0.576, 1.0)
                return mix(light_text, dark_text, self.dark_mode)
            }
        }
    }

    // Themed muted label (for section headers)
    mod.widgets.flex.ThemedLabelMuted = Label{
        draw_text +: {
            text_style: mod.widgets.flex.FONT_REGULAR{font_size: 10.0}
            dark_mode: instance(0.0)
            get_color: fn() {
                let light_text = vec4(0.557, 0.557, 0.576, 1.0)
                let dark_text = vec4(0.388, 0.388, 0.400, 1.0)
                return mix(light_text, dark_text, self.dark_mode)
            }
        }
    }

    // Primary action button (accent color)
    mod.widgets.flex.ThemedPrimaryButton = Button{
        width: Fit
        height: 32
        padding: Inset{left: 16. right: 16.}
        draw_bg +: {
            pixel: fn() {
                // Accent color stays consistent
                let normal = vec4(0.000, 0.478, 1.000, 1.0)    // #4c8bf5
                let hover_color = vec4(0.000, 0.478, 1.000, 1.0) // #6ba1ff
                let pressed_color = vec4(0.000, 0.478, 1.000, 1.0)

                let color = mix(normal, hover_color, self.hover)
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 10.0)
                        sdf.fill(mix(color, pressed_color, self.down))
                        return sdf.result
            }
        }
        draw_text +: {
            text_style: mod.widgets.flex.TEXT_BUTTON{}
            color: #xFFFFFF
        }
    }

    // Secondary button
    mod.widgets.flex.ThemedSecondaryButton = Button{
        width: Fit
        height: 32
        padding: Inset{left: 16. right: 16.}
        draw_bg +: {
            dark_mode: instance(0.0)
            pixel: fn() {
                // Light mode colors
                let light_normal = vec4(0.949, 0.949, 0.969, 1.0)  // slate-100
                let light_hover = vec4(0.898, 0.898, 0.918, 1.0)   // slate-200
                let light_pressed = vec4(0.820, 0.820, 0.839, 1.0) // slate-300
                // Dark mode colors
                let dark_normal = vec4(0.220, 0.220, 0.227, 1.0)   // #2a2a32
                let dark_hover = vec4(0.220, 0.220, 0.227, 1.0)    // #2d2d35
                let dark_pressed = vec4(0.220, 0.220, 0.227, 1.0)  // slightly lighter

                let normal = mix(light_normal, dark_normal, self.dark_mode)
                let hover_color = mix(light_hover, dark_hover, self.dark_mode)
                let pressed_color = mix(light_pressed, dark_pressed, self.dark_mode)

                let color = mix(normal, hover_color, self.hover)
                let sdf = Sdf2d.viewport(self.pos * self.rect_size)
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 10.0)
                        sdf.fill(mix(color, pressed_color, self.down))
                        return sdf.result
            }
        }
        draw_text +: {
            text_style: mod.widgets.flex.TEXT_BUTTON{}
            dark_mode: instance(0.0)
            get_color: fn() {
                let light_text = vec4(0.110, 0.110, 0.118, 1.0)
                let dark_text = vec4(0.898, 0.898, 0.918, 1.0)
                return mix(light_text, dark_text, self.dark_mode)
            }
        }
    }
}
