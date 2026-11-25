//! The visual language every DoRobot app draws with.
//!
//! The products have long claimed to "share a visual language and two widget
//! crates". Until this crate existed that was aspirational: the tokens and the
//! custom widgets lived inside one application, private to it, so the others
//! could only agree by copy-paste — and copies drift. This is that shared
//! crate.
//!
//! **It is deliberately lineage-neutral.** Both product lines depend on it —
//! the RL training console and the teleop/dataset tools — and they live in
//! separate repositories. Nothing specific to either belongs here: no engine
//! wiring, no dataset types, no screen that only one of them has. A token or a
//! widget earns its place here by being wanted by both; anything else belongs
//! in the application that wants it.
//!
//! Two modules, deliberately:
//!
//! * [`tokens`] — the iOS semantic palette, type scale and DSL
//!   prototypes (`mod.widgets.nx.*`), plus [`tokens::pal`], the Rust-side
//!   mirror for colours pushed through `script_apply_eval!`.
//! * [`kit`] — the custom-drawn widgets that no makepad primitive covers:
//!   sparkline, segmented bar, heatmap, scrub bar.
//!
//! The fonts live here rather than in either app, because
//! `crate_resource("self:resources/fonts/…")` resolves relative to the crate
//! that *defines* the DSL. A consumer needs no font files of its own.
//!
//! Register both in one call, before any widget that uses them mounts:
//!
//! ```ignore
//! fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
//!     makepad_widgets::script_mod(vm);
//!     dorobot_ux::script_mod(vm);
//!     // …then this app's own screens
//! }
//! ```

use makepad_widgets::*;

pub mod kit;
pub mod tokens;

fn ios_sheet(name: &str) -> desktop_style::StyleSheet {
    let mut sheet = desktop_style::StyleSheet::named(name).unwrap();
    // Install typography before framework controls inherit the theme. Keeping
    // it in the sheet also preserves the font across appearance reloads.
    sheet.theme.push_str(include_str!("pingfang.splash"));
    sheet.widgets.push_str(include_str!("controls.splash"));
    sheet
}

/// Select the iOS sheet before registering widgets, retaining an existing choice.
pub fn install_ios(vm: &mut ScriptVm, light: bool) {
    if desktop_style::current(vm).is_none() {
        let name = if light { "ios" } else { "ios-dark" };
        desktop_style::install(vm, ios_sheet(name));
    }
}

/// Keep framework controls and Studio's custom charts on the same appearance.
pub fn sync_appearance(cx: &mut Cx, light: bool) {
    let name = if light { "ios" } else { "ios-dark" };
    let changed = cx.with_vm(|vm| {
        // Included styles can change under JIT while the appearance name stays
        // the same. Reinstall only when their content or the appearance changes.
        if desktop_style::current(vm).is_some_and(|sheet| {
            sheet.name == name
                && sheet.widgets.ends_with(include_str!("controls.splash"))
                && sheet.theme.ends_with(include_str!("pingfang.splash"))
        }) {
            return false;
        }
        desktop_style::install(vm, ios_sheet(name));
        true
    });
    if changed {
        cx.request_style_reload();
    }
}

/// Register the tokens and the widget kit, in dependency order: `kit`'s
/// prototypes reference `nx` type styles, so the tokens must land first.
pub fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
    crate::tokens::script_mod(vm);
    crate::kit::script_mod(vm)
}
