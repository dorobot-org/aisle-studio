//! Makepad implementation of the UX design in `docs/ux/`.
//!
//! Screens are built against [`crate::api::Backend`] so they can be rendered
//! and visually validated (see `tools/vqa/`) with [`crate::api::mock::MockBackend`]
//! before any real hardware, writer, or policy backend exists.

use makepad_widgets::*;

pub mod eval;
pub mod flex_layout;
pub mod frame;
pub mod library;
pub mod nexus;
pub mod play;

/// Register every UI module. Tokens must come first — screens reference them.
pub fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
    frame::script_mod(vm);
    library::script_mod(vm);
    flex_layout::script_mod(vm);
    play::script_mod(vm);
    nexus::script_mod(vm);
    eval::script_mod(vm)
}

mod replay_scene;
