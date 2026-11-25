//! DoRobot Flex - Library
//!
//! A LeRobot dataset viewer built with Makepad.
//!
//! This library provides:
//! - Flex layout with resizable sidebars and footer
//! - Dataset loading for LeRobot v2.0/v3.0 formats
//! - Video playback (with optional FFmpeg support)
//! - Time series visualization
//! - 3D robot viewer

pub use makepad_app_shell;
pub use makepad_widgets;

pub mod api;
pub mod data;
pub mod i18n;
pub mod preferences;
pub mod runtime;
pub mod shared;
pub mod ui;
pub mod ux_probe;
pub mod widgets;

/// Still here because `ui`'s Play screen draws with it; the rest of the old
/// viewer's app modules went with its binary.
pub mod playback_controls;

/// 宿主先注册共享依赖，再注册插件自己的界面类型。
pub fn register_ui(vm: &mut makepad_widgets::ScriptVm) -> makepad_widgets::ScriptValue {
    dorobot_ux::install_ios(vm, !std::env::args().any(|a| a == "--dark"));
    makepad_widgets::script_mod(vm);
    dorobot_ux::script_mod(vm);
    makepad_urdf_player::makepad_xr::script_mod(vm);
    makepad_urdf_player::script_mod(vm);
    makepad_app_shell::script_mod_with_theme(vm, |vm| {
        shared::shell_theme::script_mod(vm);
    });
    shared::script_mod(vm);
    widgets::script_mod(vm);
    playback_controls::script_mod(vm);
    ui::script_mod(vm)
}
