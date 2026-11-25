use super::*;
use dorobot_studio_core::survey::asset::{Asset, Markup, Origin, Pixel, Shape, Unit};

const AXES: [&str; 3] = ["Width", "Depth", "Height"];

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Tool {
    #[default]
    Outline,
    Floor,
    Ruler,
}
impl Tool {
    fn points(self, m: &mut Markup, axis: usize) -> &mut Vec<Pixel> {
        match self {
            Self::Outline => &mut m.outline,
            Self::Floor => &mut m.floor,
            Self::Ruler => &mut m.rulers[axis],
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Self::Outline => "Click the outline vertices. Drag a point to adjust it.",
            Self::Floor => "Click floor reference points. Drag a point to adjust it.",
            Self::Ruler => "Mark two endpoints, enter their known distance and choose Apply.",
        }
    }
}

#[derive(Default)]
pub(super) struct AssetEditor {
    pub open: bool,
    pub axis: usize,
    pub asset: Asset,
    unit: Unit,
    tool: Tool,
    physics: bool,
    dirty: bool,
    drag: Option<usize>,
    rotation: usize,
}

impl SurveyPane {
    fn note(&self, cx: &mut Cx, text: &str) {
        self.view.label(cx, ids!(asset_feedback)).set_text(cx, text);
    }
    fn field(&self, cx: &Cx, id: &[LiveId]) -> String {
        self.view.text_input(cx, id).text().trim().to_string()
    }
    fn load_asset(&mut self, cx: &mut Cx, asset: Asset) {
        self.editor.asset = asset;
        self.editor.axis = 0;
        self.editor.tool = Tool::Outline;
        self.editor.dirty = false;
        self.editor.drag = None;
        let a = &self.editor.asset;
        for (id, text) in [
            (ids!(asset_name) as &[LiveId], a.name.clone()),
            (ids!(category), a.category.clone()),
            (ids!(material), a.physics.material.value.clone()),
            (ids!(surface_name), a.physics.surface_name.value.clone()),
            (ids!(friction), a.physics.friction.value.to_string()),
            (ids!(restitution), a.physics.restitution.value.to_string()),
            (
                ids!(mass),
                a.physics
                    .mass_kg
                    .value
                    .map(|m| m.to_string())
                    .unwrap_or_default(),
            ),
            (
                ids!(top_thickness),
                a.physics.top_thickness_m.value.to_string(),
            ),
            (ids!(leg_width), a.physics.leg_width_m.value.to_string()),
        ] {
            self.view.text_input(cx, id).set_text(cx, &text);
        }
        self.axis_fields(cx);
        self.editor_labels(cx);
        self.show(cx);
    }
    fn axis_fields(&mut self, cx: &mut Cx) {
        let dim = &self.editor.asset.dimensions[self.editor.axis];
        self.editor.unit = dim.as_ref().map(|d| d.unit).unwrap_or_default();
        self.view.text_input(cx, ids!(length_value)).set_text(
            cx,
            &dim.as_ref()
                .map(|d| d.value.to_string())
                .unwrap_or_default(),
        );
        self.view
            .button(cx, ids!(length_unit))
            .set_text(cx, self.editor.unit.label());
    }
    fn editor_labels(&self, cx: &mut Cx) {
        let e = &self.editor;
        let a = &e.asset;
        let label = if a.revision == 0 {
            "New asset".to_string()
        } else {
            format!("{} · revision {}", a.name, a.revision)
        };
        self.view
            .label(cx, ids!(asset_version))
            .set_text(cx, &label);
        self.view
            .label(cx, ids!(axis_label))
            .set_text(cx, AXES[e.axis]);
        self.view
            .label(cx, ids!(mark_hint))
            .set_text(cx, e.tool.hint());
        self.view
            .widget(cx, ids!(marks_panel))
            .set_visible(cx, !e.physics);
        self.view
            .widget(cx, ids!(physics_panel))
            .set_visible(cx, e.physics);
        self.view
            .widget(cx, ids!(mass_row))
            .set_visible(cx, a.physics.movable.value);
        self.view
            .widget(cx, ids!(table_details))
            .set_visible(cx, a.physics.shape == Shape::FourLegTable);
        self.view
            .button(cx, ids!(appearance))
            .set_text(cx, &format!("Appearance · {}", a.physics.appearance.value));
        self.view.button(cx, ids!(asset_shape)).set_text(
            cx,
            match a.physics.shape {
                Shape::Box => "Shape · box",
                Shape::FourLegTable => "Shape · table",
            },
        );
        self.view.button(cx, ids!(graspable)).set_text(
            cx,
            if a.physics.graspable.value {
                "Graspable"
            } else {
                "Enable grasping"
            },
        );
        self.view.button(cx, ids!(movable)).set_text(
            cx,
            if a.physics.movable.value {
                "Movable"
            } else {
                "Fixed"
            },
        );
        for (id, selected) in [
            (ids!(outline) as &[LiveId], e.tool == Tool::Outline),
            (ids!(floor_ref), e.tool == Tool::Floor),
            (ids!(dimension), e.tool == Tool::Ruler),
        ] {
            self.view.button(cx, id).set_enabled(cx, !selected);
        }
        let values = a
            .dimensions
            .iter()
            .zip(AXES)
            .map(|(d, axis)| match d {
                Some(d) => format!(
                    "{axis}   {} {} · {}",
                    d.value,
                    d.unit.label(),
                    match d.provenance {
                        Origin::Measured { .. } => "measured",
                        Origin::Operator => "entered",
                        Origin::Default => "default",
                    }
                ),
                None => format!("{axis}   Unmeasured"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        self.view
            .label(cx, ids!(measured_values))
            .set_text(cx, &values);
        self.view
            .widget(cx, ids!(geometry_panel))
            .set_visible(cx, e.open && e.physics);
        self.view
            .widget(cx, ids!(photo_canvas))
            .set_visible(cx, !e.open || !e.physics);
        if e.physics {
            let player = VideoPlayerWidgetRefExt::as_video_player(
                &self.view.widget(cx, ids!(geometry_view)),
            );
            match a.calculate_geometry() {
                Ok(g) if !g.colliders.is_empty() => {
                    let image = g.preview(e.rotation);
                    player.set_frame(
                        cx,
                        image.width() as usize,
                        image.height() as usize,
                        image.as_raw(),
                    );
                    self.view.label(cx, ids!(geometry_info)).set_text(
                        cx,
                        &format!(
                            "{} colliders · {}\n{}",
                            g.colliders.len(),
                            g.surfaces
                                .first()
                                .map(|s| s.name.as_str())
                                .unwrap_or("No placement surface"),
                            values
                        ),
                    );
                }
                result => {
                    player.clear(cx);
                    player.set_placeholder_text(cx, "Set width, depth and height");
                    self.view.label(cx, ids!(geometry_info)).set_text(
                        cx,
                        &result.err().map(|e| e.to_string()).unwrap_or_else(|| {
                            "Set width, depth and height for a metric preview".into()
                        }),
                    );
                }
            }
        }
    }
    fn select_asset(&mut self, cx: &mut Cx, move_by: i32, blank: bool) {
        if self.editor.dirty {
            self.note(
                cx,
                "Save this revision or discard its edits before switching assets.",
            );
            return;
        }
        match project().assets() {
            Ok(assets) => {
                let selected = assets.iter().position(|a| a.id == self.editor.asset.id);
                let asset = if blank || assets.is_empty() {
                    Asset::default()
                } else {
                    let i = selected
                        .map(|i| (i as i32 + move_by).rem_euclid(assets.len() as i32) as usize)
                        .unwrap_or(0);
                    assets[i].clone()
                };
                self.note(cx, "");
                self.load_asset(cx, asset);
            }
            Err(err) => self.note(cx, &err.to_string()),
        }
    }
    fn physics_fields(
        &self,
        cx: &Cx,
    ) -> Result<dorobot_studio_core::survey::asset::PhysicalAsset, String> {
        let mut p = self.editor.asset.physics.clone();
        let number = |id: &[LiveId], name: &str| {
            self.field(cx, id)
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("Enter a finite {name}"))
        };
        p.material.edit(self.field(cx, ids!(material)));
        p.friction.edit(number(ids!(friction), "friction")?);
        p.restitution
            .edit(number(ids!(restitution), "restitution")?);
        if p.movable.value {
            p.mass_kg.edit(Some(number(ids!(mass), "mass in kg")?));
        }
        if p.shape == Shape::FourLegTable {
            p.surface_name.edit(self.field(cx, ids!(surface_name)));
            p.top_thickness_m
                .edit(number(ids!(top_thickness), "top thickness in metres")?);
            p.leg_width_m
                .edit(number(ids!(leg_width), "leg width in metres")?);
        }
        Ok(p)
    }
    fn save_revision(&mut self, cx: &mut Cx) {
        let mut a = self.editor.asset.clone();
        a.name = self.field(cx, ids!(asset_name));
        a.category = self.field(cx, ids!(category));
        match self.physics_fields(cx) {
            Ok(p) => a.physics = p,
            Err(msg) => {
                self.note(cx, &msg);
                return;
            }
        }
        if let Some(p) = self.photos.get(self.index) {
            a.photos.entry(p.id.clone()).or_default();
        }
        match project().save_asset(&a) {
            Ok(saved) => {
                self.load_asset(cx, saved);
                self.note(cx, "Saved asset revision");
            }
            Err(err) => self.note(cx, &err.to_string()),
        }
    }

    pub(super) fn handle_editor(&mut self, cx: &mut Cx, event: &Event, actions: &Actions) {
        let clicked = |id: &[LiveId]| self.view.button(cx, id).clicked(actions);
        if clicked(ids!(edit_assets)) {
            self.editor.open = !self.editor.open;
            self.view
                .widget(cx, ids!(asset_inspector))
                .set_visible(cx, self.editor.open);
            self.view.button(cx, ids!(edit_assets)).set_text(
                cx,
                if self.editor.open {
                    "Close editor"
                } else {
                    "Edit assets"
                },
            );
            if self.editor.open && self.editor.asset.id.is_empty() && !self.editor.dirty {
                self.select_asset(cx, 0, false);
            } else {
                self.editor_labels(cx);
                self.show(cx);
            }
            return;
        }
        if !self.editor.open {
            return;
        }
        if clicked(ids!(discard_edits)) {
            self.editor.dirty = false;
            self.select_asset(cx, 0, self.editor.asset.id.is_empty());
            return;
        }
        if clicked(ids!(new_asset)) {
            self.select_asset(cx, 0, true);
            return;
        }
        if clicked(ids!(next_asset)) {
            self.select_asset(cx, 1, false);
            return;
        }
        if clicked(ids!(older_asset)) {
            self.select_asset(cx, -1, false);
            return;
        }
        if clicked(ids!(save_asset)) {
            self.save_revision(cx);
            return;
        }
        if clicked(ids!(edit_marks)) || clicked(ids!(edit_physics)) {
            self.editor.physics = clicked(ids!(edit_physics));
            self.editor_labels(cx);
            return;
        }
        if clicked(ids!(preview_physics)) {
            match self.physics_fields(cx) {
                Ok(physics) => {
                    self.editor.asset.physics = physics;
                    self.editor_labels(cx);
                }
                Err(e) => self.note(cx, &e),
            }
            return;
        }
        if clicked(ids!(rotate_geometry)) {
            self.editor.rotation = (self.editor.rotation + 1) % 4;
            self.editor_labels(cx);
            return;
        }
        if clicked(ids!(appearance)) {
            let shades = ["neutral", "oak", "red", "green", "blue"];
            let current = &mut self.editor.asset.physics.appearance;
            let next = shades
                .iter()
                .position(|s| *s == current.value)
                .map_or(0, |i| (i + 1) % shades.len());
            current.edit(shades[next].to_string());
            self.editor.dirty = true;
            self.editor_labels(cx);
            return;
        }
        if clicked(ids!(graspable)) {
            let p = &mut self.editor.asset.physics.graspable;
            p.edit(!p.value);
            self.editor.dirty = true;
            self.editor_labels(cx);
            return;
        }
        if clicked(ids!(movable)) {
            let p = &mut self.editor.asset.physics.movable;
            p.edit(!p.value);
            self.editor.dirty = true;
            self.editor_labels(cx);
            return;
        }
        if clicked(ids!(asset_shape)) {
            let p = &mut self.editor.asset.physics;
            p.shape = if p.shape == Shape::Box {
                Shape::FourLegTable
            } else {
                Shape::Box
            };
            self.editor.dirty = true;
            self.editor_labels(cx);
            return;
        }
        if clicked(ids!(next_axis)) || clicked(ids!(previous_axis)) {
            self.editor.axis =
                (self.editor.axis + if clicked(ids!(next_axis)) { 1 } else { 2 }) % 3;
            self.editor.drag = None;
            self.axis_fields(cx);
            self.editor_labels(cx);
            self.show(cx);
            return;
        }
        if clicked(ids!(length_unit)) {
            self.editor.unit = self.editor.unit.next();
            self.view
                .button(cx, ids!(length_unit))
                .set_text(cx, self.editor.unit.label());
            return;
        }
        for (id, tool) in [
            (ids!(outline) as &[LiveId], Tool::Outline),
            (ids!(floor_ref), Tool::Floor),
            (ids!(dimension), Tool::Ruler),
        ] {
            if clicked(id) {
                self.editor.tool = tool;
                self.editor.drag = None;
                self.editor_labels(cx);
                return;
            }
        }
        if clicked(ids!(apply_length)) {
            let Some(photo) = self.photos.get(self.index) else {
                self.note(cx, "Select a photo");
                return;
            };
            let value = self
                .field(cx, ids!(length_value))
                .parse()
                .unwrap_or(f64::NAN);
            match self
                .editor
                .asset
                .set_dimension(self.editor.axis, value, self.editor.unit, photo)
            {
                Ok(()) => {
                    self.editor.dirty = true;
                    self.editor_labels(cx);
                    self.note(cx, "Length applied to draft");
                }
                Err(e) => self.note(cx, &e.to_string()),
            }
            return;
        }
        let clear = clicked(ids!(clear_marks));
        if [
            ids!(asset_name) as &[LiveId],
            ids!(category),
            ids!(material),
            ids!(friction),
            ids!(restitution),
            ids!(mass),
            ids!(top_thickness),
            ids!(leg_width),
            ids!(surface_name),
        ]
        .iter()
        .any(|id| self.view.text_input(cx, id).changed(actions).is_some())
        {
            self.editor.dirty = true;
        }
        let Some(photo) = self.photos.get(self.index) else {
            return;
        };
        if clear {
            if let Some(m) = self.editor.asset.photos.get_mut(&photo.id) {
                self.editor.tool.points(m, self.editor.axis).clear();
                if self.editor.tool==Tool::Ruler && self.editor.asset.dimensions[self.editor.axis].as_ref().is_some_and(|d|matches!(&d.provenance,Origin::Measured {photo:id,..} if id==&photo.id)){
                    self.editor.asset.dimensions[self.editor.axis]=None;self.axis_fields(cx);
                }
                self.editor.dirty = true;
                self.editor_labels(cx);
                self.show(cx);
            }
            return;
        }
        if self.editor.physics {
            return;
        }
        let area = self
            .view
            .widget(cx, ids!(photo_canvas.video_area.frame_image))
            .area();
        let rect = area.rect(cx);
        if rect.size.x <= 0. || rect.size.y <= 0. {
            return;
        }
        let to_pixel = |abs: Vec2d| {
            [
                ((abs.x - rect.pos.x) / rect.size.x * f64::from(photo.width))
                    .clamp(0., f64::from(photo.width - 1)),
                ((abs.y - rect.pos.y) / rect.size.y * f64::from(photo.height))
                    .clamp(0., f64::from(photo.height - 1)),
            ]
        };
        let (is_down, pixel) = match event.hits(cx, area) {
            Hit::FingerDown(e) => (true, to_pixel(e.abs)),
            Hit::FingerMove(e) if self.editor.drag.is_some() => (false, to_pixel(e.abs)),
            Hit::FingerUp(_) => {
                self.editor.drag = None;
                return;
            }
            _ => return,
        };
        let mark = self
            .editor
            .asset
            .photos
            .entry(photo.id.clone())
            .or_default();
        let points = self.editor.tool.points(mark, self.editor.axis);
        if is_down {
            let nearest = points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    (
                        i,
                        ((p[0] - pixel[0]) * rect.size.x / f64::from(photo.width))
                            .hypot((p[1] - pixel[1]) * rect.size.y / f64::from(photo.height)),
                    )
                })
                .filter(|(_, distance)| *distance <= 12.)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, _)) = nearest {
                self.editor.drag = Some(i);
                return;
            }
            if self.editor.tool == Tool::Ruler && points.len() == 2 {
                self.note(cx, "Drag an endpoint, or clear this ruler to draw another.");
                return;
            }
            points.push(pixel);
            self.editor.drag = Some(points.len() - 1);
        } else if let Some(i) = self.editor.drag {
            if let Some(p) = points.get_mut(i) {
                *p = pixel;
            }
        }
        self.editor.dirty = true;
        self.show_photo(cx);
    }
}
