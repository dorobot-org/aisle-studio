//! The nexus palette, handed to `makepad-app-shell` through its theme hook.
//!
//! app-shell draws its chrome — header, sidebar, panel bodies and title bars,
//! footer — from `mod.widgets.shell.*` tokens bound as uniform defaults. This
//! module replaces those tokens with dorobot-ux's surfaces, so the shell and
//! the screens inside it read as one product.
//!
//! It must be registered inside `script_mod_with_theme`'s hook: after
//! app-shell's defaults, before its widgets bind them. Registering it anywhere
//! else compiles and silently does nothing.
//!
//! **Both halves of every pair are set.** app-shell mixes `_L` toward `_D` on
//! its `dark_mode` uniform, and this app's own widgets already carry the same
//! dorobot-ux pairs, so with both halves filled the whole window — chrome and
//! content — moves together on one switch instead of one of them being pinned.

use makepad_widgets::*;

script_mod! {
    use mod.prelude.widgets.*

    // dorobot-ux surfaces. The ladder is void (ground) -> deep (surface) ->
    // lift (raised), with edge for hairlines; see `dorobot_ux::tokens::pal`.

    // Ground everything sits on — ux void.
    mod.widgets.shell.BG_CONTENT_L = #xF2F2F7
    mod.widgets.shell.BG_CONTENT_D = #x000000

    // Chrome surfaces — ux deep.
    mod.widgets.shell.BG_HEADER_L = #xFFFFFF
    mod.widgets.shell.BG_HEADER_D = #x1C1C1E
    mod.widgets.shell.BG_PANEL_L = #xFFFFFF
    mod.widgets.shell.BG_PANEL_D = #x1C1C1E
    mod.widgets.shell.BG_FOOTER_L = #xFFFFFF
    mod.widgets.shell.BG_FOOTER_D = #x1C1C1E

    // Raised surfaces — ux lift. The sidebar and a panel's title bar sit a
    // step above the surface they belong to.
    mod.widgets.shell.BG_SIDEBAR_L = #xFFFFFF
    mod.widgets.shell.BG_SIDEBAR_D = #x2C2C2E
    mod.widgets.shell.BG_PANEL_TITLE_L = #xFFFFFF
    mod.widgets.shell.BG_PANEL_TITLE_D = #x2C2C2E

    // Hairlines — ux edge.
    mod.widgets.shell.BORDER_L = #xD1D1D6
    mod.widgets.shell.BORDER_D = #x38383A

    // Type — ux ink and dim.
    mod.widgets.shell.TEXT_L = #x1C1C1E
    mod.widgets.shell.TEXT_D = #xF5F5F7
    mod.widgets.shell.TEXT_DIM_L = #x636366
    mod.widgets.shell.TEXT_DIM_D = #xAEAEB2

    // Accent — ux vio, the iOS blue accent.
    mod.widgets.shell.ACCENT_L = #x007AFF
    mod.widgets.shell.ACCENT_D = #x0A84FF
}
