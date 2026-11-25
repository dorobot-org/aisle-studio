use dorobot_studio_plugin::*;

struct Example;
impl StudioPlugin for Example {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: "example",
            title: "Example",
            api_version: 1,
            panels: &[PanelDescriptor {
                id: "example.panel",
                title: "Example panel",
                legacy_screen: None,
            }],
        }
    }
    fn register(&self, _: &mut ScriptVm) {}
    fn create(&self, _: &str, _: &mut ScriptVm) -> Result<Box<dyn PluginPanel>, String> {
        Err("没有界面工厂".into())
    }
}

#[test]
fn duplicate_plugin_does_not_replace_the_registered_factory() {
    let mut registry = PluginRegistry::default();
    registry.register(Box::new(Example)).unwrap();
    assert!(registry.register(Box::new(Example)).is_err());
    assert_eq!(registry.descriptors().len(), 1);
    assert_eq!(registry.panel_owner("example.panel"), Some("example"));
}

#[test]
fn unknown_panel_cannot_be_selected() {
    let mut registry = PluginRegistry::default();
    registry.register(Box::new(Example)).unwrap();
    assert!(registry.select("missing.panel").is_err());
    registry.select("example.panel").unwrap();
    assert_eq!(registry.selected(), Some("example.panel"));
}

#[test]
fn disabled_plugin_cannot_be_opened_and_can_be_reenabled() {
    let mut registry = PluginRegistry::default();
    registry.register(Box::new(Example)).unwrap();
    let backend = dorobot_studio_core::api::mock::MockBackend::new();
    registry.set_enabled("example", false, &backend).unwrap();
    assert!(registry.select("example.panel").is_err());
    registry.set_enabled("example", true, &backend).unwrap();
    registry.select("example.panel").unwrap();
}

struct Other;
impl StudioPlugin for Other {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: "other",
            title: "Other",
            api_version: 1,
            panels: &[PanelDescriptor {
                id: "example.panel",
                title: "Collision",
                legacy_screen: None,
            }],
        }
    }
    fn register(&self, _: &mut ScriptVm) {}
    fn create(&self, _: &str, _: &mut ScriptVm) -> Result<Box<dyn PluginPanel>, String> {
        unreachable!()
    }
}
#[test]
fn panel_id_collision_is_rejected_without_partial_registration() {
    let mut registry = PluginRegistry::default();
    registry.register(Box::new(Example)).unwrap();
    assert!(registry.register(Box::new(Other)).is_err());
    assert_eq!(registry.descriptors().len(), 1);
    assert_eq!(registry.panel_owner("example.panel"), Some("example"));
}
