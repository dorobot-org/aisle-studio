use super::*;
use dorobot_studio_core::survey::scene::{RuleProperty, SceneRule};

impl SceneEditorPane {
    pub(super) fn rule_fields(&self, cx: &mut Cx) {
        let scene = self.visible_scene();
        let choices = self.view.drop_down(cx, ids!(rule_list));
        choices.set_labels(
            cx,
            std::iter::once("New range".to_owned())
                .chain(scene.rules.iter().map(|r| r.name.clone()))
                .collect(),
        );
        let i = scene
            .rules
            .iter()
            .position(|r| self.rule_selected.as_ref() == Some(&r.name));
        choices.set_selected_item(cx, i.map_or(0, |n| n + 1));
        let r = i.map(|n| &scene.rules[n]);
        let kind = r.map_or(RuleProperty::Ambient, |r| r.property);
        let properties = self.view.drop_down(cx, ids!(rule_property));
        properties.set_labels(
            cx,
            RuleProperty::ALL.iter().map(|p| p.label().into()).collect(),
        );
        properties.set_selected_item(
            cx,
            RuleProperty::ALL
                .iter()
                .position(|p| *p == kind)
                .unwrap_or(0),
        );
        let targets = self.view.drop_down(cx, ids!(rule_target));
        targets.set_labels(cx, scene.instances.iter().map(|v| v.name.clone()).collect());
        let obj = r
            .and_then(|r| r.instance.as_ref())
            .or(self.selected.as_ref());
        targets.set_selected_item(
            cx,
            scene
                .instances
                .iter()
                .position(|v| Some(&v.id) == obj)
                .unwrap_or(0),
        );
        self.view
            .widget(cx, ids!(rule_target_row))
            .set_visible(cx, kind != RuleProperty::Ambient);
        self.view
            .text_input(cx, ids!(rule_name))
            .set_text(cx, r.map_or("", |r| r.name.as_str()));
        for (j, id) in [ids!(rule_min) as &[LiveId], ids!(rule_max)]
            .into_iter()
            .enumerate()
        {
            self.view
                .text_input(cx, id)
                .set_text(cx, &r.map(|r| r.range[j].to_string()).unwrap_or_default());
        }
        self.view
            .button(cx, ids!(rule_delete))
            .set_enabled(cx, r.is_some() && self.candidate.is_none());
        self.view
            .button(cx, ids!(rule_apply))
            .set_enabled(cx, self.candidate.is_none());
    }

    pub(super) fn rule_actions(&mut self, cx: &mut Cx, a: &Actions) -> Result<bool, String> {
        if let Some(i) = self.view.drop_down(cx, ids!(rule_list)).selected(a) {
            self.rule_selected = i
                .checked_sub(1)
                .and_then(|n| self.editor.scene.rules.get(n))
                .map(|r| r.name.clone());
            self.rule_fields(cx);
            return Ok(true);
        }
        if let Some(i) = self.view.drop_down(cx, ids!(rule_property)).selected(a) {
            self.view
                .widget(cx, ids!(rule_target_row))
                .set_visible(cx, RuleProperty::ALL.get(i) != Some(&RuleProperty::Ambient));
            return Ok(true);
        }
        let delete = self.view.button(cx, ids!(rule_delete)).clicked(a);
        if !delete && !self.view.button(cx, ids!(rule_apply)).clicked(a) {
            return Ok(false);
        }
        let mut scene = self.editor.scene.clone();
        let selected = scene
            .rules
            .iter()
            .position(|r| Some(&r.name) == self.rule_selected.as_ref());
        if delete {
            scene.rules.remove(selected.ok_or("Choose a saved range")?);
            self.rule_selected = None;
        } else {
            let name = self.text(cx, ids!(rule_name));
            if scene
                .rules
                .iter()
                .enumerate()
                .any(|(i, r)| Some(i) != selected && r.name == name)
            {
                return Err("Use a unique range name".into());
            }
            let property = *RuleProperty::ALL
                .get(self.view.drop_down(cx, ids!(rule_property)).selected_item())
                .ok_or("Select a range property")?;
            let instance = if property == RuleProperty::Ambient {
                None
            } else {
                Some(
                    scene
                        .instances
                        .get(self.view.drop_down(cx, ids!(rule_target)).selected_item())
                        .ok_or("Choose an object for the range")?
                        .id
                        .clone(),
                )
            };
            let rule = SceneRule {
                name: name.clone(),
                property,
                instance,
                range: [
                    self.number(cx, ids!(rule_min), "minimum")?,
                    self.number(cx, ids!(rule_max), "maximum")?,
                ],
            };
            rule.check(&scene).map_err(|e| e.to_string())?;
            if let Some(i) = selected {
                scene.rules[i] = rule;
            } else {
                scene.rules.push(rule);
            }
            self.rule_selected = Some(name);
        }
        self.commit(cx, scene);
        self.note(
            cx,
            "Save the scene revision to make its ranges available in Run",
        );
        Ok(true)
    }
}
