//! Editable metric scenes. Instances pin the asset revision used for geometry and physics.
use super::{asset::Asset, Survey};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, HashSet}, fs, io::Write};

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Revision {
    pub id: String,
    pub revision: u64,
}

impl Revision {
    fn check(&self) -> Result<()> {
        ensure!(
            self.revision > 0
                && !self.id.is_empty()
                && self.id.len() <= 80
                && self
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-'),
            "Invalid library reference"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub asset: Revision,
    /// World floor coordinates and the height of the bottom of the object.
    pub position_m: [f64; 3],
    pub yaw_deg: f64,
    #[serde(default)]
    pub support: Option<validation::Support>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_area: Option<StartArea>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StartArea {
    /// None selects the room floor. Coordinates are relative to the support centre and yaw.
    pub support: Option<validation::Support>,
    /// Local XY minimum and maximum in metres. None selects the complete surface.
    pub bounds_m: Option<[[f64; 2]; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleProperty {
    X,
    Y,
    Z,
    Yaw,
    Friction,
    Ambient,
}

impl RuleProperty {
    pub const ALL: [Self; 6] = [
        Self::X,
        Self::Y,
        Self::Z,
        Self::Yaw,
        Self::Friction,
        Self::Ambient,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::X => "World X (m)",
            Self::Y => "World Y (m)",
            Self::Z => "World Z (m)",
            Self::Yaw => "Yaw (°)",
            Self::Friction => "Surface friction",
            Self::Ambient => "Room ambient light",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneRule {
    pub name: String,
    pub property: RuleProperty,
    pub instance: Option<String>,
    pub range: [f64; 2],
}
impl SceneRule {
    pub fn check(&self, scene: &Scene) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty() && self.name == self.name.trim(),
            "Enter a rule name"
        );
        let [lo, hi] = self.range;
        ensure!(
            lo.is_finite() && hi.is_finite() && lo <= hi,
            "Minimum must be at or below maximum"
        );
        if self.property == RuleProperty::Ambient {
            ensure!(
                self.instance.is_none() && lo >= 0. && hi <= 1.,
                "Ambient light needs a room range from 0 to 1"
            );
        } else {
            ensure!(
                scene
                    .instances
                    .iter()
                    .any(|i| self.instance.as_ref() == Some(&i.id)),
                "Choose an existing object for the rule"
            );
            ensure!(
                self.property != RuleProperty::Friction || lo >= 0.,
                "Friction must be nonnegative"
            );
        }
        Ok(())
    }
}

/// Poses are expressed in the room frame, with translations in metres and angles in degrees.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Registration {
    /// X, Y and counterclockwise heading of the robot on the floor.
    pub robot_xy_yaw: [f64; 3],
    /// Room-from-camera X, Y, Z, roll, pitch and yaw (extrinsic XYZ).
    pub fixed_cameras: BTreeMap<String, [f64; 6]>,
}

impl Registration {
    fn is_default(&self) -> bool { self == &Self::default() }

    pub fn check(&self, room: [f64; 2]) -> Result<()> {
        let [x, y, yaw] = self.robot_xy_yaw;
        ensure!(x.is_finite() && y.is_finite() && yaw.is_finite()
            && x.abs() < room[0] / 2. && y.abs() < room[1] / 2., "Set the robot start inside the room");
        for (name, pose) in &self.fixed_cameras {
            ensure!(!name.trim().is_empty() && !name.chars().any(char::is_whitespace)
                && pose.iter().all(|v| v.is_finite()) && pose[2] >= 0.,
                "Each fixed camera needs a name, finite coordinates and a height above the floor");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub revision: u64,
    pub parent: Option<Revision>,
    pub name: String,
    pub room_m: [f64; 2],
    #[serde(default, skip_serializing_if = "Registration::is_default")]
    pub registration: Registration,
    pub instances: Vec<Instance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<SceneRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edit_history: Vec<agent_edit::EditSource>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            id: String::new(),
            revision: 0,
            parent: None,
            name: "Untitled room".into(),
            room_m: [6., 4.],
            registration: Registration::default(),
            instances: vec![],
            rules: vec![],
            edit_history: vec![],
        }
    }
}

impl Scene {
    pub fn reference(&self) -> Option<Revision> {
        (self.revision > 0).then(|| Revision {
            id: self.id.clone(),
            revision: self.revision,
        })
    }

    pub fn place(&mut self, asset: &Asset, position_m: [f64; 3]) -> Result<String> {
        let pin = Revision {
            id: asset.id.clone(),
            revision: asset.revision,
        };
        pin.check()?;
        ensure!(position_m.iter().all(|v| v.is_finite()), "Invalid position");
        let id = format!("object-{:032x}", fastrand::u128(..));
        let mut name = asset.name.clone();
        let mut n = 2;
        while self.instances.iter().any(|item| item.name == name) {
            name = format!("{} {n}", asset.name);
            n += 1;
        }
        self.instances.push(Instance {
            id: id.clone(),
            name,
            asset: pin,
            position_m,
            yaw_deg: 0.,
            support: None,
            start_area: None,
        });
        Ok(id)
    }

    fn check(&self) -> Result<()> {
        ensure!(!self.name.trim().is_empty(), "Name the scene");
        ensure!(
            self.room_m.iter().all(|n| n.is_finite() && *n > 0.),
            "Room dimensions must be positive and finite"
        );
        self.registration.check(self.room_m)?;
        let mut ids = HashSet::new();
        for item in &self.instances {
            ensure!(
                !item.id.is_empty() && ids.insert(&item.id),
                "Scene instance IDs must be unique"
            );
            ensure!(!item.name.trim().is_empty(), "Name each scene object");
            ensure!(
                item.position_m.iter().all(|n| n.is_finite()) && item.yaw_deg.is_finite(),
                "Invalid object pose"
            );
            item.asset.check()?;
        }
        let mut names = HashSet::new();
        for rule in &self.rules {
            rule.check(self)?;
            ensure!(names.insert(&rule.name), "Scene rule names must be unique");
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct SceneEditor {
    pub scene: Scene,
    back: Vec<Scene>,
    forward: Vec<Scene>,
}

impl SceneEditor {
    pub fn open(&mut self, scene: Scene) {
        self.scene = scene;
        self.back.clear();
        self.forward.clear();
    }

    pub fn change(&mut self, next: Scene) {
        if next != self.scene {
            self.back.push(std::mem::replace(&mut self.scene, next));
            self.forward.clear();
        }
    }

    fn restore(&mut self, mut layout: Scene) -> Scene {
        // History edits the content of the current document, including after Save As.
        layout.id.clone_from(&self.scene.id);
        layout.revision = self.scene.revision;
        layout.parent.clone_from(&self.scene.parent);
        std::mem::replace(&mut self.scene, layout)
    }

    pub fn undo(&mut self) {
        if let Some(previous) = self.back.pop() {
            let current = self.restore(previous);
            self.forward.push(current);
        }
    }
    pub fn redo(&mut self) {
        if let Some(next) = self.forward.pop() {
            let current = self.restore(next);
            self.back.push(current);
        }
    }
}

impl Survey {
    pub fn asset_revision(&self, pin: &Revision) -> Result<Asset> {
        pin.check()?;
        let path = self
            .root
            .join("assets")
            .join(&pin.id)
            .join(format!("{}.json", pin.revision));
        let asset: Asset =
            serde_json::from_slice(&fs::read(path).context("Pinned asset revision is missing")?)?;
        ensure!(
            asset.id == pin.id && asset.revision == pin.revision,
            "Pinned asset identity changed"
        );
        Ok(asset)
    }

    pub fn scene_revision(&self, pin: &Revision) -> Result<Scene> {
        pin.check()?;
        let path = self
            .root
            .join("scenes")
            .join(&pin.id)
            .join(format!("{}.json", pin.revision));
        let scene: Scene =
            serde_json::from_slice(&fs::read(path).context("Scene revision is missing")?)?;
        ensure!(
            scene.reference().as_ref() == Some(pin),
            "Scene revision identity changed"
        );
        scene.check()?;
        Ok(scene)
    }

    pub fn scenes(&self) -> Result<Vec<Scene>> {
        let root = self.root.join("scenes");
        if !root.exists() {
            return Ok(vec![]);
        }
        let mut scenes = vec![];
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let mut latest = 0;
            for file in fs::read_dir(entry.path())? {
                let path = file?.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(n) = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        latest = latest.max(n);
                    }
                }
            }
            if latest > 0 {
                scenes.push(self.scene_revision(&Revision {
                    id: entry.file_name().to_string_lossy().into_owned(),
                    revision: latest,
                })?);
            }
        }
        scenes.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
        Ok(scenes)
    }

    pub fn save_scene(&self, draft: &Scene, save_as: bool) -> Result<Scene> {
        draft.check()?;
        for pin in draft
            .instances
            .iter()
            .map(|i| &i.asset)
            .collect::<HashSet<_>>()
        {
            self.asset_revision(pin)?;
        }
        let root = self.root.join("scenes");
        fs::create_dir_all(&root)?;
        let mut saved = draft.clone();
        saved.name = saved.name.trim().to_string();
        let dir = if let Some(pin) = draft.reference() {
            self.scene_revision(&pin)?;
            if save_as {
                saved.parent = Some(pin);
                saved.revision = 0;
                saved.id.clear();
            }
            root.join(&saved.id)
        } else {
            ensure!(draft.id.is_empty(), "New scenes receive a new identity");
            root.clone()
        };
        let dir = if saved.revision == 0 {
            let dir = tempfile::Builder::new()
                .prefix("scene-")
                .tempdir_in(&root)?
                .keep();
            saved.id = dir.file_name().unwrap().to_string_lossy().into_owned();
            dir
        } else {
            dir
        };
        saved.revision = saved
            .revision
            .checked_add(1)
            .context("Invalid scene revision")?;
        let mut file = tempfile::NamedTempFile::new_in(&dir)?;
        file.write_all(&serde_json::to_vec_pretty(&saved)?)?;
        file.as_file().sync_all()?;
        match file.persist_noclobber(dir.join(format!("{}.json", saved.revision))) {
            Ok(_) => Ok(saved),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                bail!("Another session saved a new revision. Reopen the scene before saving")
            }
            Err(e) => Err(e.error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_ranges_preserve_legacy_scene_bytes_and_invalid_ranges_cannot_be_saved() -> Result<()> {
        let mut scene = Scene::default();
        let original = serde_json::to_string(&scene)?;
        assert!(!original.contains("\"rules\""));
        assert_eq!(
            serde_json::to_string(&serde_json::from_str::<Scene>(&original)?)?,
            original
        );
        let rule = SceneRule {
            name: "Daylight".into(),
            property: RuleProperty::Ambient,
            instance: None,
            range: [0.2, 0.8],
        };
        scene.rules.push(rule.clone());
        scene.check()?;
        scene.rules.push(rule);
        assert!(scene.check().is_err());
        scene.rules.pop();
        scene.rules[0].range = [0.2, 1.2];
        assert!(scene.check().is_err());
        scene.rules[0].property = RuleProperty::Friction;
        scene.rules[0].instance = Some("missing-table".into());
        assert!(scene.check().is_err());
        Ok(())
    }

    #[test]
    fn two_chairs_keep_independent_poses_and_revision_pins_across_undo_and_save_as() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let lib = Survey::new(tmp.path().into());
        let chair = lib.save_asset(&Asset {
            name: "Chair".into(),
            ..Default::default()
        })?;
        let mut editor = SceneEditor::default();
        let mut layout = editor.scene.clone();
        let left = layout.place(&chair, [-1., 0., 0.])?;
        let right = layout.place(&chair, [1., 0., 0.])?;
        assert_ne!(left, right);
        editor.change(layout);
        let base = lib.save_scene(&editor.scene, false)?;
        editor.scene = base.clone();
        let mut layout = editor.scene.clone();
        layout.instances.remove(0);
        editor.change(layout);
        editor.undo();
        assert_eq!(editor.scene, base);
        editor.redo();
        let mut layout = editor.scene.clone();
        layout.name = "One chair".into();
        editor.change(layout);
        let variant = lib.save_scene(&editor.scene, true)?;
        assert_ne!(variant.id, base.id);
        assert_eq!(variant.parent, base.reference());
        assert_eq!(variant.instances[0].id, right);
        assert_eq!(lib.scene_revision(&base.reference().unwrap())?, base);
        let later = lib.save_scene(&base, false)?;
        assert_eq!(later.revision, 2);
        assert!(lib.save_scene(&base, false).is_err());
        assert_eq!(lib.scenes()?.len(), 2);
        lib.save_asset(&Asset {
            name: "Renamed chair".into(),
            ..chair.clone()
        })?;
        assert_eq!(lib.asset_revision(&variant.instances[0].asset)?, chair);
        Ok(())
    }
}

pub mod agent_edit;
pub mod composition;
pub mod viewport;

pub mod validation;
