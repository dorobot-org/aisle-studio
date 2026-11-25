//! Editable survey assets. Each save publishes a self-contained, immutable revision.
use super::{Photo, Survey};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{ErrorKind, Write},
};

pub type Pixel = [f64; 2];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Markup {
    pub outline: Vec<Pixel>,
    pub floor: Vec<Pixel>,
    /// Endpoints in the oriented source image, indexed by width, depth, height.
    pub rulers: [Vec<Pixel>; 3],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Mm,
    #[default]
    Cm,
    M,
    In,
}
impl Unit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mm => "mm",
            Self::Cm => "cm",
            Self::M => "m",
            Self::In => "in",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Mm => Self::Cm,
            Self::Cm => Self::M,
            Self::M => Self::In,
            Self::In => Self::Mm,
        }
    }
    fn metres(self) -> f64 {
        match self {
            Self::Mm => 0.001,
            Self::Cm => 0.01,
            Self::M => 1.,
            Self::In => 0.0254,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum Origin {
    Default,
    Operator,
    Measured {
        photo: String,
        endpoints: [Pixel; 2],
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub value: f64,
    pub unit: Unit,
    pub provenance: Origin,
}
impl Dimension {
    pub fn metres(&self) -> f64 {
        self.value * self.unit.metres()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Property<T> {
    pub value: T,
    pub provenance: Origin,
}
impl<T: PartialEq> Property<T> {
    fn default_value(value: T) -> Self {
        Self {
            value,
            provenance: Origin::Default,
        }
    }
    pub fn edit(&mut self, value: T) {
        if self.value != value {
            self.value = value;
            self.provenance = Origin::Operator;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    #[default]
    Box,
    FourLegTable,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicalAsset {
    pub material: Property<String>,
    pub appearance: Property<String>,
    pub friction: Property<f64>,
    pub restitution: Property<f64>,
    pub movable: Property<bool>,
    pub graspable: Property<bool>,
    pub surface_name: Property<String>,
    pub mass_kg: Property<Option<f64>>,
    pub shape: Shape,
    pub top_thickness_m: Property<f64>,
    pub leg_width_m: Property<f64>,
}
impl Default for PhysicalAsset {
    fn default() -> Self {
        Self {
            material: Property::default_value("Unspecified".into()),
            appearance: Property::default_value("neutral".into()),
            friction: Property::default_value(0.6),
            restitution: Property::default_value(0.),
            movable: Property::default_value(false),
            graspable: Property::default_value(false),
            surface_name: Property::default_value("tabletop".into()),
            mass_kg: Property::default_value(None),
            shape: Shape::Box,
            top_thickness_m: Property::default_value(0.035),
            leg_width_m: Property::default_value(0.045),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Collider {
    pub name: String,
    pub center_m: [f64; 3],
    pub size_m: [f64; 3],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Surface {
    pub name: String,
    pub center_m: [f64; 3],
    pub size_m: [f64; 2],
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub colliders: Vec<Collider>,
    pub surfaces: Vec<Surface>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub revision: u64,
    pub name: String,
    pub category: String,
    pub photos: BTreeMap<String, Markup>,
    pub dimensions: [Option<Dimension>; 3],
    pub physics: PhysicalAsset,
    pub geometry: Geometry,
}
impl Asset {
    pub fn set_dimension(
        &mut self,
        axis: usize,
        value: f64,
        unit: Unit,
        photo: &Photo,
    ) -> Result<()> {
        ensure!(
            axis < 3 && value.is_finite() && value > 0.,
            "Enter a positive known length"
        );
        let points = self.photos.entry(photo.id.clone()).or_default();
        let provenance = match points.rulers[axis].as_slice() {
            [] => Origin::Operator,
            [a, b] if a != b => Origin::Measured {
                photo: photo.id.clone(),
                endpoints: [*a, *b],
            },
            _ => bail!("Mark two distinct endpoints before applying a measured length"),
        };
        self.dimensions[axis] = Some(Dimension {
            value,
            unit,
            provenance,
        });
        Ok(())
    }

    pub fn calculate_geometry(&self) -> Result<Geometry> {
        ensure!(
            self.dimensions
                .iter()
                .flatten()
                .all(|dim| dim.metres().is_finite() && dim.metres() > 0.),
            "Dimensions must be positive and finite"
        );
        if self.physics.shape == Shape::FourLegTable {
            ensure!(
                !self.physics.surface_name.value.trim().is_empty(),
                "Name the placement surface"
            );
        }
        let [Some(width), Some(depth), Some(height)] = &self.dimensions else {
            return Ok(Geometry::default());
        };
        let (w, d, h) = (width.metres(), depth.metres(), height.metres());
        let mut geometry = Geometry::default();
        let mut add = |name: &str, center_m, size_m| {
            geometry.colliders.push(Collider {
                name: name.into(),
                center_m,
                size_m,
            })
        };
        match self.physics.shape {
            Shape::Box => add("body", [0., 0., h / 2.], [w, d, h]),
            Shape::FourLegTable => {
                let slab = self.physics.top_thickness_m.value;
                let leg = self.physics.leg_width_m.value;
                ensure!(
                    slab > 0. && slab < h && leg > 0. && 2. * leg < w.min(d),
                    "Tabletop thickness or leg width leaves no open space"
                );
                add("top", [0., 0., h - slab / 2.], [w, d, slab]);
                for (name, sx, sy) in [
                    ("leg-front-left", 1., 1.),
                    ("leg-front-right", 1., -1.),
                    ("leg-back-left", -1., 1.),
                    ("leg-back-right", -1., -1.),
                ] {
                    add(
                        name,
                        [sx * (w - leg) / 2., sy * (d - leg) / 2., (h - slab) / 2.],
                        [leg, leg, h - slab],
                    );
                }
                geometry.surfaces.push(Surface {
                    name: self.physics.surface_name.value.trim().into(),
                    center_m: [0., 0., h],
                    size_m: [w, d],
                });
            }
        }
        Ok(geometry)
    }
}

impl Survey {
    fn asset_path(&self, id: &str) -> Result<std::path::PathBuf> {
        ensure!(
            !id.is_empty()
                && id.len() <= 80
                && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
            "Invalid survey asset ID"
        );
        Ok(self.root.join("assets").join(id))
    }

    pub fn assets(&self) -> Result<Vec<Asset>> {
        let root = self.root.join("assets");
        if !root.exists() {
            return Ok(vec![]);
        }
        let mut latest = Vec::new();
        for dir in fs::read_dir(root)? {
            let dir = dir?;
            if !dir.file_type()?.is_dir() {
                continue;
            }
            let mut rev = 0;
            for file in fs::read_dir(dir.path())? {
                let path = file?.path();
                if path.extension().and_then(|x| x.to_str()) == Some("json") {
                    if let Some(n) = path
                        .file_stem()
                        .and_then(|x| x.to_str())
                        .and_then(|x| x.parse::<u64>().ok())
                    {
                        rev = rev.max(n);
                    }
                }
            }
            if rev > 0 {
                let asset: Asset =
                    serde_json::from_slice(&fs::read(dir.path().join(format!("{rev}.json")))?)?;
                ensure!(
                    asset.revision == rev && dir.file_name().to_str() == Some(&asset.id),
                    "Asset revision identity changed"
                );
                latest.push(asset);
            }
        }
        latest.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
        Ok(latest)
    }

    pub fn save_asset(&self, draft: &Asset) -> Result<Asset> {
        ensure!(
            !draft.name.trim().is_empty(),
            "Name the asset before saving"
        );
        let p = &draft.physics;
        ensure!(
            p.friction.value.is_finite()
                && p.friction.value >= 0.
                && (0.0..=1.0).contains(&p.restitution.value),
            "Check friction and restitution"
        );
        if p.movable.value {
            ensure!(
                p.mass_kg.value.is_some_and(|v| v.is_finite() && v > 0.),
                "Enter a positive mass for a movable asset"
            );
        }
        for (id, mark) in &draft.photos {
            ensure!(
                id.len() == 64 && id.bytes().all(|c| c.is_ascii_hexdigit()),
                "Invalid source photo ID"
            );
            let photo: Photo = serde_json::from_slice(
                &fs::read(self.photos().join(id).join("photo.json"))
                    .context("Source photo is missing")?,
            )?;
            for point in mark
                .outline
                .iter()
                .chain(&mark.floor)
                .chain(mark.rulers.iter().flatten())
            {
                ensure!(
                    (0.0..f64::from(photo.width)).contains(&point[0])
                        && (0.0..f64::from(photo.height)).contains(&point[1]),
                    "A marked point is outside the source photo"
                );
            }
            for line in &mark.rulers {
                ensure!(line.len() <= 2, "A dimension line has two endpoints");
            }
        }
        let mut saved = draft.clone();
        saved.name = saved.name.trim().into();
        for (axis, dim) in saved.dimensions.iter_mut().enumerate() {
            if let Some(Dimension {
                provenance: Origin::Measured { photo, endpoints },
                ..
            }) = dim
            {
                let [a, b] = saved
                    .photos
                    .get(photo)
                    .context("Measured dimension is missing its photo")?
                    .rulers[axis]
                    .as_slice()
                else {
                    bail!("Measured dimension needs two endpoints");
                };
                ensure!(a != b, "Measured endpoints coincide");
                *endpoints = [*a, *b];
            }
        }
        saved.geometry = saved.calculate_geometry()?;
        let root = self.root.join("assets");
        fs::create_dir_all(&root)?;
        let dir = if saved.revision == 0 {
            ensure!(saved.id.is_empty(), "New assets receive a new ID");
            let path = tempfile::Builder::new()
                .prefix("asset-")
                .tempdir_in(&root)?
                .keep();
            saved.id = path.file_name().unwrap().to_string_lossy().into_owned();
            path
        } else {
            let path = self.asset_path(&saved.id)?;
            ensure!(
                path.join(format!("{}.json", saved.revision)).is_file(),
                "The base revision is missing"
            );
            path
        };
        saved.revision = saved
            .revision
            .checked_add(1)
            .context("Invalid asset revision")?;
        let mut file = tempfile::NamedTempFile::new_in(&dir)?;
        file.write_all(&serde_json::to_vec_pretty(&saved)?)?;
        file.as_file().sync_all()?;
        match file.persist_noclobber(dir.join(format!("{}.json", saved.revision))) {
            Ok(_) => Ok(saved),
            Err(e) if e.error.kind() == ErrorKind::AlreadyExists => {
                bail!("Another session saved a new revision. Reopen the asset before saving")
            }
            Err(e) => Err(e.error.into()),
        }
    }
}

impl Markup {
    /// Render only into a disposable preview; all stored points remain in original image pixels.
    pub fn paint(&self, preview: &mut image::RgbImage, photo: &Photo, axis: usize) {
        let scale = [
            preview.width() as f64 / photo.width as f64,
            preview.height() as f64 / photo.height as f64,
        ];
        let mut stroke = |points: &[Pixel], color: [u8; 3], closed: bool| {
            let pixels: Vec<_> = points
                .iter()
                .map(|p| {
                    [
                        (p[0] * scale[0]).round() as i32,
                        (p[1] * scale[1]).round() as i32,
                    ]
                })
                .collect();
            let mut dot = |x: i32, y: i32, r: i32, c: [u8; 3]| {
                for dy in -r..=r {
                    for dx in -r..=r {
                        if dx * dx + dy * dy <= r * r {
                            if let Some(pixel) =
                                preview.get_pixel_mut_checked((x + dx) as u32, (y + dy) as u32)
                            {
                                *pixel = image::Rgb(c);
                            }
                        }
                    }
                }
            };
            for (a, b) in pixels.iter().zip(
                pixels
                    .iter()
                    .skip(1)
                    .chain(pixels.first().filter(|_| closed && pixels.len() > 2)),
            ) {
                let dx = b[0] - a[0];
                let dy = b[1] - a[1];
                let n = dx.abs().max(dy.abs()).max(1);
                for i in 0..=n {
                    dot(a[0] + dx * i / n, a[1] + dy * i / n, 2, color);
                }
            }
            for p in pixels {
                dot(p[0], p[1], 6, color);
                dot(p[0], p[1], 2, [255, 255, 255]);
            }
        };
        stroke(&self.outline, [0, 174, 195], true);
        for p in &self.floor {
            stroke(std::slice::from_ref(p), [160, 79, 224], false);
        }
        for (i, r) in self.rulers.iter().enumerate() {
            stroke(
                r,
                if i == axis {
                    [245, 132, 26]
                } else {
                    [169, 104, 55]
                },
                false,
            );
        }
    }
}

impl Geometry {
    /// Orthographic engineering view of the same metric boxes saved with this revision.
    pub fn preview(&self, rotation: usize) -> image::RgbImage {
        let mut out = image::RgbImage::from_pixel(400, 200, image::Rgb([241, 243, 246]));
        let az = std::f64::consts::PI * (0.21 + rotation as f64 / 2.0);
        let (s, c) = az.sin_cos();
        let (se, ce) = 0.46_f64.sin_cos();
        let mut faces = vec![];
        for body in &self.colliders {
            let mut vertices = [[0.; 3]; 8];
            for (i, vertex) in vertices.iter_mut().enumerate() {
                let [x, y, z] = std::array::from_fn(|axis| {
                    body.center_m[axis]
                        + body.size_m[axis] * if (i >> axis) & 1 == 1 { 0.5 } else { -0.5 }
                });
                let depth = x * c + y * s;
                *vertex = [-x * s + y * c, depth * se - z * ce, depth * ce + z * se];
            }
            for (indices, shade) in [
                ([4, 5, 7, 6], 0),
                (if c > 0. { [1, 3, 7, 5] } else { [0, 2, 6, 4] }, 1),
                (if s > 0. { [2, 3, 7, 6] } else { [0, 1, 5, 4] }, 2),
            ] {
                let face = indices.map(|i| vertices[i]);
                let depth = face.iter().map(|v| v[2]).sum::<f64>() / 4.;
                let rgb = match (body.name.as_str(), shade) {
                    ("top", 0) => [45, 159, 167],
                    (_, 0) => [104, 151, 196],
                    (_, 1) => [75, 105, 136],
                    _ => [110, 137, 161],
                };
                faces.push((depth, face, rgb));
            }
        }
        if faces.is_empty() {
            return out;
        }
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for (_, face, _) in &faces {
            for p in face {
                for k in 0..2 {
                    min[k] = min[k].min(p[k]);
                    max[k] = max[k].max(p[k]);
                }
            }
        }
        let zoom = (368. / (max[0] - min[0]).max(0.0001)).min(174. / (max[1] - min[1]).max(0.0001));
        faces.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, face, rgb) in faces {
            let verts = face.map(|v| {
                [
                    (v[0] - (min[0] + max[0]) / 2.) * zoom + 200.,
                    (v[1] - (min[1] + max[1]) / 2.) * zoom + 100.,
                ]
            });
            for y in 0..out.height() {
                let line = y as f64 + 0.5;
                let mut hits = vec![];
                for (a, b) in verts.iter().zip(verts.iter().cycle().skip(1)) {
                    if (a[1] <= line && line < b[1]) || (b[1] <= line && line < a[1]) {
                        hits.push(a[0] + (line - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
                    }
                }
                if let [a, b] = hits.as_slice() {
                    for x in (a.min(*b).ceil() as u32)..(a.max(*b).ceil() as u32).min(out.width()) {
                        out.put_pixel(x, y, image::Rgb(rgb));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metric_box_and_table_use_photo_units_and_pin_each_saved_revision() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let survey = Survey::new(tmp.path().join("project"));
        let path = tmp.path().join("photo.png");
        image::RgbImage::new(400, 300).save(&path)?;
        let photo = survey.import(&path)?;
        let mut a = Asset {
            name: "Table".into(),
            ..Default::default()
        };
        a.physics.shape = Shape::FourLegTable;
        let endpoints = [[10., 11.], [390., 12.]];
        a.photos.entry(photo.id.clone()).or_default().rulers[0] = endpoints.to_vec();
        a.set_dimension(0, 800., Unit::Mm, &photo)?;
        a.set_dimension(1, 50., Unit::Cm, &photo)?;
        a.set_dimension(2, 0.45, Unit::M, &photo)?;
        let first = survey.save_asset(&a)?;
        assert_eq!(first.geometry.colliders.len(), 5);
        let support = &first.geometry.surfaces[0];
        assert_eq!(
            (support.name.as_str(), support.center_m, support.size_m),
            ("tabletop", [0., 0., 0.45], [0.8, 0.5])
        );
        let mut edited = first.clone();
        let endpoint = [290., 90.];
        edited.photos.get_mut(&photo.id).unwrap().rulers[0][1] = endpoint;
        let second = survey.save_asset(&edited)?;
        assert_eq!(second.revision, 2);
        assert_eq!(
            second.dimensions[0].as_ref().unwrap().provenance,
            Origin::Measured {
                photo: photo.id,
                endpoints: [endpoints[0], endpoint]
            }
        );
        assert_eq!(survey.assets()?, vec![second]);
        assert!(survey
            .save_asset(&first)
            .unwrap_err()
            .to_string()
            .contains("Another session"));
        let original: Asset = serde_json::from_reader(fs::File::open(
            survey.asset_path(&first.id)?.join("1.json"),
        )?)?;
        assert_eq!(original, first);
        Ok(())
    }

    #[test]
    fn movable_asset_requires_mass_and_unknown_dimensions_stay_unknown() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let survey = Survey::new(tmp.path().into());
        let mut a = Asset {
            name: "Cube".into(),
            ..Default::default()
        };
        a.physics.movable.edit(true);
        assert!(survey
            .save_asset(&a)
            .unwrap_err()
            .to_string()
            .contains("mass"));
        a.physics.mass_kg.edit(Some(0.09));
        let saved = survey.save_asset(&a)?;
        assert!(
            saved.geometry.colliders.is_empty() && saved.dimensions.iter().all(Option::is_none)
        );
        assert_eq!(saved.physics.mass_kg.provenance, Origin::Operator);
        Ok(())
    }
}
