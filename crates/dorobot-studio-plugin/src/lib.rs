//! 编译时链接的 Rust 原生插件契约；不将 Rust trait 当作稳定动态库 ABI。
use dorobot_studio_core::api::{Backend, Screen};
pub use makepad_widgets::*;

pub const API_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug)]
pub struct PanelDescriptor {
    pub id: &'static str,
    pub title: &'static str,
    /// 迁移现有页面时使用；新面板可以通过工具箱直接选择。
    pub legacy_screen: Option<Screen>,
}

#[derive(Clone, Copy, Debug)]
pub struct PluginDescriptor {
    pub id: &'static str,
    pub title: &'static str,
    pub api_version: u32,
    pub panels: &'static [PanelDescriptor],
}

pub struct PanelContext<'a> {
    pub backend: &'a dyn Backend,
    pub rig_editing: bool,
    pub show_help: bool,
    pub show_details: bool,
    pub live_controls: bool,
}

/// 描述与工厂不持有界面状态；每次 create 返回独立实例。
pub trait StudioPlugin {
    fn descriptor(&self) -> PluginDescriptor;
    fn register(&self, vm: &mut ScriptVm);
    fn create(&self, panel: &str, vm: &mut ScriptVm) -> Result<Box<dyn PluginPanel>, String>;
    fn can_disable(&self, _backend: &dyn Backend) -> Result<(), String> {
        Ok(())
    }
}

pub trait PluginPanel {
    fn root(&self) -> WidgetRef;
    fn sync(&mut self, cx: &mut Cx, context: &PanelContext<'_>);
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions, backend: &mut dyn Backend)
        -> bool;
    fn on_disable(&mut self, _cx: &mut Cx) {}
    fn visible_in(&self, _context: &PanelContext<'_>) -> Option<bool> {
        None
    }
}

struct RegisteredPlugin {
    factory: Box<dyn StudioPlugin>,
    enabled: bool,
}
struct MountedPanel {
    owner: &'static str,
    descriptor: PanelDescriptor,
    instance: Box<dyn PluginPanel>,
    visible: bool,
}

struct NavigationButton {
    widget: WidgetRef,
    id: LiveId,
    owner: &'static str,
    panel: Option<PanelDescriptor>,
}

#[derive(Default)]
pub struct PluginRegistry {
    plugins: Vec<RegisteredPlugin>,
    mounted: Vec<MountedPanel>,
    selected: Option<String>,
    toolbox_buttons: Vec<NavigationButton>,
    tab_buttons: Vec<NavigationButton>,
    recent_panels: std::collections::HashMap<&'static str, &'static str>,
}

impl PluginRegistry {
    pub fn register(&mut self, factory: Box<dyn StudioPlugin>) -> Result<(), String> {
        let descriptor = factory.descriptor();
        if descriptor.id.is_empty() || descriptor.api_version != API_VERSION {
            return Err(format!("插件标识或 API 版本无效：{}", descriptor.id));
        }
        if self
            .plugins
            .iter()
            .any(|p| p.factory.descriptor().id == descriptor.id)
        {
            return Err(format!("重复插件：{}", descriptor.id));
        }
        let mut seen = std::collections::HashSet::new();
        let mut screens = std::collections::HashSet::new();
        for panel in descriptor.panels {
            if panel.id.is_empty() || !seen.insert(panel.id) || self.panel_owner(panel.id).is_some()
            {
                return Err(format!("重复或无效面板：{}", panel.id));
            }
            if panel.legacy_screen.is_some_and(|screen| {
                !screens.insert(screen)
                    || self
                        .descriptors()
                        .iter()
                        .any(|d| d.panels.iter().any(|p| p.legacy_screen == Some(screen)))
            }) {
                return Err("已有插件拥有该页面".into());
            }
        }
        self.plugins.push(RegisteredPlugin {
            factory,
            enabled: true,
        });
        Ok(())
    }

    pub fn descriptors(&self) -> Vec<PluginDescriptor> {
        self.plugins
            .iter()
            .map(|p| p.factory.descriptor())
            .collect()
    }
    pub fn panel_owner(&self, panel: &str) -> Option<&'static str> {
        self.descriptors()
            .into_iter()
            .find(|d| d.panels.iter().any(|p| p.id == panel))
            .map(|d| d.id)
    }
    pub fn enabled(&self, plugin: &str) -> bool {
        self.plugins
            .iter()
            .any(|p| p.enabled && p.factory.descriptor().id == plugin)
    }
    pub fn set_enabled(
        &mut self,
        plugin: &str,
        enabled: bool,
        backend: &dyn Backend,
    ) -> Result<(), String> {
        let index = self
            .plugins
            .iter()
            .position(|p| p.factory.descriptor().id == plugin)
            .ok_or_else(|| format!("未知插件：{plugin}"))?;
        if !enabled && self.plugins[index].enabled {
            self.plugins[index].factory.can_disable(backend)?;
        }
        self.plugins[index].enabled = enabled;
        if !enabled
            && self
                .selected
                .as_deref()
                .is_some_and(|id| self.panel_owner(id) == Some(plugin))
        {
            self.selected = None;
        }
        Ok(())
    }
    pub fn select(&mut self, panel: &str) -> Result<(), String> {
        let owner = self
            .panel_owner(panel)
            .ok_or_else(|| format!("未知面板：{panel}"))?;
        if !self.enabled(owner) {
            return Err(format!("插件已禁用：{owner}"));
        }
        self.selected = Some(panel.into());
        Ok(())
    }
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    pub fn follow_navigation(&mut self) {
        self.selected = None;
    }
    pub fn screen_available(&self, screen: Screen) -> bool {
        self.descriptors()
            .iter()
            .find(|d| d.panels.iter().any(|p| p.legacy_screen == Some(screen)))
            .is_none_or(|d| self.enabled(d.id))
    }
    pub fn register_widgets(&self, vm: &mut ScriptVm) {
        for plugin in &self.plugins {
            plugin.factory.register(vm);
        }
    }
    pub fn create_instance(
        &self,
        panel: &str,
        vm: &mut ScriptVm,
    ) -> Result<Box<dyn PluginPanel>, String> {
        let owner = self
            .panel_owner(panel)
            .ok_or_else(|| format!("未知面板：{panel}"))?;
        if !self.enabled(owner) {
            return Err(format!("插件已禁用：{owner}"));
        }
        self.plugins
            .iter()
            .find(|p| p.factory.descriptor().id == owner)
            .unwrap()
            .factory
            .create(panel, vm)
    }

    /// 将默认实例挂到已有容器；保留旧页面标识以兼容布局与验收工具。
    pub fn mount(&mut self, cx: &mut Cx, container: &WidgetRef) -> Result<(), String> {
        let descriptors = self.descriptors();
        for plugin in descriptors {
            for descriptor in plugin.panels {
                if let Some(mounted) = self
                    .mounted
                    .iter()
                    .find(|p| p.descriptor.id == descriptor.id)
                {
                    let id = LiveId::from_str(descriptor.id);
                    let mut parent = container
                        .borrow_mut::<View>()
                        .ok_or("插件挂载点不是 View")?;
                    if !parent.children.iter().any(|(child, _)| *child == id) {
                        parent.children.push((id, mounted.instance.root()));
                    }
                    continue;
                }
                // 禁用只控制可用性；仍创建界面以便当前进程中重新启用。
                let factory = &self
                    .plugins
                    .iter()
                    .find(|p| p.factory.descriptor().id == plugin.id)
                    .unwrap()
                    .factory;
                let instance = cx.with_vm(|vm| factory.create(descriptor.id, vm))?;
                let widget = instance.root();
                if widget.is_empty() {
                    return Err(format!("插件未创建界面：{}", descriptor.id));
                }
                let mut parent = container
                    .borrow_mut::<View>()
                    .ok_or("插件挂载点不是 View")?;
                if parent
                    .children
                    .iter()
                    .any(|(id, _)| *id == LiveId::from_str(descriptor.id))
                {
                    return Err(format!("面板挂载点已占用：{}", descriptor.id));
                }
                widget.set_visible(cx, false);
                parent
                    .children
                    .push((LiveId::from_str(descriptor.id), widget));
                self.mounted.push(MountedPanel {
                    owner: plugin.id,
                    descriptor: *descriptor,
                    instance,
                    visible: false,
                });
            }
        }
        cx.widget_tree_mark_dirty(container.widget_uid());
        container.redraw(cx);
        Ok(())
    }

    /// 侧栏仅显示插件；其面板由右侧顶部标签栏呈现。
    pub fn mount_toolbox(&mut self, cx: &mut Cx, container: &WidgetRef) -> Result<(), String> {
        for descriptor in self.descriptors() {
            if self
                .toolbox_buttons
                .iter()
                .any(|b| b.owner == descriptor.id)
            {
                continue;
            }
            let widget = cx.with_vm(|vm| {
                let value = script_eval!(vm, {
                    use mod.prelude.widgets.* use mod.widgets.*
                    mod.widgets.ux.QuietButton{width: Fill height: 40 min_height: 40
                        align: Align{x: 0.0 y: 0.5} padding: Inset{left: 12. right: 12.}
                        draw_text +: {text_style: mod.widgets.ux.TEXT_NAV{}}}
                });
                WidgetRef::script_from_value(vm, value)
            });
            widget.set_text(cx, descriptor.title);
            self.toolbox_buttons.push(NavigationButton {
                widget,
                id: LiveId::from_str(&format!("toolbox_{}", descriptor.id)),
                owner: descriptor.id,
                panel: None,
            });
        }
        Self::mount_navigation(cx, container, &self.toolbox_buttons)
    }

    pub fn mount_tabs(&mut self, cx: &mut Cx, container: &WidgetRef) -> Result<(), String> {
        for plugin in self.descriptors() {
            for descriptor in plugin.panels {
                if self
                    .tab_buttons
                    .iter()
                    .any(|b| b.panel.is_some_and(|p| p.id == descriptor.id))
                {
                    continue;
                }
                let widget = cx.with_vm(|vm| {
                    let value = script_eval!(vm, {
                        use mod.prelude.widgets.* use mod.widgets.*
                        mod.widgets.ux.WorkspaceTab{}
                    });
                    WidgetRef::script_from_value(vm, value)
                });
                widget.set_text(cx, descriptor.title);
                widget.set_visible(cx, false);
                self.tab_buttons.push(NavigationButton {
                    widget,
                    id: LiveId::from_str(&format!("plugin_tab_{}", descriptor.id)),
                    owner: plugin.id,
                    panel: Some(*descriptor),
                });
            }
        }
        Self::mount_navigation(cx, container, &self.tab_buttons)
    }

    fn mount_navigation(
        cx: &mut Cx,
        container: &WidgetRef,
        buttons: &[NavigationButton],
    ) -> Result<(), String> {
        let mut parent = container
            .borrow_mut::<View>()
            .ok_or("导航挂载点不是 View")?;
        for button in buttons {
            if !parent.children.iter().any(|(id, _)| *id == button.id) {
                parent.children.push((button.id, button.widget.clone()));
            }
        }
        drop(parent);
        cx.widget_tree_mark_dirty(container.widget_uid());
        container.redraw(cx);
        Ok(())
    }
    pub fn toolbox_selection(&self, _cx: &mut Cx, actions: &Actions) -> Option<PanelDescriptor> {
        let owner = self
            .toolbox_buttons
            .iter()
            .find(|b| self.enabled(b.owner) && b.widget.as_button().clicked(actions))?
            .owner;
        let plugin = self.descriptors().into_iter().find(|d| d.id == owner)?;
        plugin
            .panels
            .iter()
            .find(|p| self.recent_panels.get(owner) == Some(&p.id))
            .or_else(|| plugin.panels.first())
            .copied()
    }

    pub fn tab_selection(&self, actions: &Actions) -> Option<PanelDescriptor> {
        self.tab_buttons
            .iter()
            .find(|b| {
                self.enabled(b.owner) && b.widget.visible() && b.widget.as_button().clicked(actions)
            })
            .and_then(|b| b.panel)
    }

    pub fn active_panel(&self, screen: Screen) -> Option<PanelDescriptor> {
        self.descriptors()
            .into_iter()
            .filter(|d| self.enabled(d.id))
            .flat_map(|d| d.panels.iter().copied())
            .find(|p| match self.selected() {
                Some(id) => p.id == id,
                None => p.legacy_screen == Some(screen),
            })
    }

    pub fn active_plugin(&self, screen: Screen) -> Option<PluginDescriptor> {
        let panel = self.active_panel(screen)?;
        self.descriptors()
            .into_iter()
            .find(|d| d.panels.iter().any(|p| p.id == panel.id))
    }

    pub fn sync_navigation(&mut self, cx: &mut Cx, screen: Screen) {
        let active = self.active_panel(screen);
        let owner = active.and_then(|p| self.panel_owner(p.id));
        if let (Some(owner), Some(panel)) = (owner, active) {
            self.recent_panels.insert(owner, panel.id);
        }
        let light = dorobot_studio_core::ui::frame::light_mode();
        for button in &self.toolbox_buttons {
            button
                .widget
                .as_button()
                .set_enabled(cx, self.enabled(button.owner));
            dorobot_studio_core::ui::frame::sync_nav_button(
                cx,
                &button.widget,
                owner == Some(button.owner),
                light,
            );
        }
        for button in &self.tab_buttons {
            button.widget.set_visible(cx, owner == Some(button.owner));
            dorobot_studio_core::ui::frame::sync_nav_button(
                cx,
                &button.widget,
                active.is_some_and(|p| button.panel.is_some_and(|tab| tab.id == p.id)),
                light,
            );
        }
    }

    /// 动态创建的根不在宿主 DSL 中，主题重载时单独重放它们的源对象。
    pub fn reapply(&mut self, cx: &mut Cx) {
        cx.with_vm(|vm| {
            for panel in &self.mounted {
                let mut root = panel.instance.root();
                let source = root.script_source();
                if source != ScriptObject::ZERO {
                    root.script_apply(
                        vm,
                        &Apply::ScriptReapply,
                        &mut Scope::empty(),
                        source.into(),
                    );
                }
            }
            for button in self.toolbox_buttons.iter().chain(&self.tab_buttons) {
                let mut button = button.widget.clone();
                let source = button.script_source();
                if source != ScriptObject::ZERO {
                    button.script_apply(
                        vm,
                        &Apply::ScriptReapply,
                        &mut Scope::empty(),
                        source.into(),
                    );
                }
            }
        });
    }

    pub fn sync(&mut self, cx: &mut Cx, context: &PanelContext<'_>) {
        let enabled: Vec<_> = self
            .plugins
            .iter()
            .filter(|p| p.enabled)
            .map(|p| p.factory.descriptor().id)
            .collect();
        for panel in &mut self.mounted {
            let active = enabled.contains(&panel.owner)
                && match &self.selected {
                    Some(selected) => selected == panel.descriptor.id,
                    None => panel.instance.visible_in(context).unwrap_or(
                        panel.descriptor.legacy_screen == Some(context.backend.screen()),
                    ),
                };
            if panel.visible && !active {
                panel.instance.on_disable(cx);
            }
            panel.visible = active;
            panel.instance.root().set_visible(cx, active);
            if active {
                panel.instance.sync(cx, context);
            }
        }
    }
    pub fn handle_actions(
        &mut self,
        cx: &mut Cx,
        actions: &Actions,
        backend: &mut dyn Backend,
    ) -> bool {
        let mut changed = false;
        for panel in &mut self.mounted {
            if panel.visible {
                changed |= panel.instance.handle_actions(cx, actions, backend);
            }
        }
        changed
    }
}
