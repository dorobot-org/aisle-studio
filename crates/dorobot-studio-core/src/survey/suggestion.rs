//! Operator decisions on model proposals are retained with the physical asset revision.
use super::{
    asset::{Asset, Dimension, Origin, Pixel, Shape, Unit},
    Photo,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub photo: String,
    pub photo_sha256: String,
    pub provider: String,
    pub model: String,
    pub input_mode: String,
    pub request_sha256: String,
    pub outline: Vec<Pixel>,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flat_reference: Option<FlatReference>,
    pub fields: Vec<Field>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlatReference {
    pub axis: usize,
    pub length_m: f64,
    pub endpoints: [Pixel; 2],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub field: String,
    pub value: String,
    #[serde(default)]
    pub status: Decision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<String>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    #[default]
    Pending,
    Accepted,
    Corrected,
    Discarded,
}

impl Suggestion {
    pub fn validate(&self, photo: &Photo) -> Result<()> {
        ensure!(
            self.photo == photo.id && self.photo_sha256 == photo.sha256,
            "Suggestion source photo changed"
        );
        ensure!(
            self.request_sha256.len() == 64
                && self.request_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                && matches!(self.provider.as_str(), "codex" | "octos-deepseek")
                && !self.model.is_empty(),
            "Suggestion is missing its provider receipt"
        );
        let on_photo = |p: &Pixel| {
            (0.0..f64::from(photo.width)).contains(&p[0])
                && (0.0..f64::from(photo.height)).contains(&p[1])
        };
        ensure!(
            self.outline.len() >= 3 && self.outline.iter().all(on_photo),
            "Proposed outline is outside the source photo"
        );
        let axes = ["width", "depth", "height"];
        if let Some(r) = &self.flat_reference {
            let [a, b] = r.endpoints;
            ensure!(
                r.axis < axes.len()
                    && r.length_m.is_finite()
                    && r.length_m > 0.
                    && r.endpoints.iter().all(on_photo)
                    && (a[0] - b[0]).hypot(a[1] - b[1]) >= 2.,
                "Invalid planar ruler"
            );
        }
        let mut keys = std::collections::HashSet::new();
        ensure!(
            !self.fields.is_empty(),
            "No supported properties in this proposal"
        );
        for f in &self.fields {
            let valid = |value: &str| match f.field.as_str() {
                "name" => !value.trim().is_empty() && value.chars().count() <= 100,
                "shape" => matches!(value, "box" | "four_leg_table"),
                key => {
                    self.flat_reference
                        .as_ref()
                        .is_some_and(|r| axes[r.axis] == key)
                        && value.parse::<f64>().is_ok_and(|n| n.is_finite() && n > 0.)
                }
            };
            ensure!(
                keys.insert(&f.field) && valid(&f.value),
                "Invalid proposed property"
            );
            ensure!(
                match f.status {
                    Decision::Pending | Decision::Discarded => f.applied.is_none(),
                    Decision::Accepted => f.applied.as_ref() == Some(&f.value),
                    Decision::Corrected => f
                        .applied
                        .as_deref()
                        .is_some_and(|v| v != f.value && valid(v)),
                },
                "Invalid recorded property decision"
            );
        }
        Ok(())
    }
}
impl Asset {
    pub fn review_field(
        &mut self,
        proposal: usize,
        index: usize,
        edit: Option<&str>,
    ) -> Result<Decision> {
        let suggestion = self
            .suggestions
            .get(proposal)
            .context("Choose a proposal")?;
        let field = suggestion.fields.get(index).context("Choose a property")?;
        ensure!(
            field.status == Decision::Pending,
            "This property already has a review decision"
        );
        let decision = match edit {
            None => Decision::Discarded,
            Some(value) if value.trim() == field.value => Decision::Accepted,
            Some(_) => Decision::Corrected,
        };
        let applied = edit.map(|v| v.trim().to_owned());
        if let Some(value) = &applied {
            let origin = if decision == Decision::Corrected {
                Origin::Operator
            } else {
                Origin::Suggested {
                    photo: suggestion.photo.clone(),
                    provider: suggestion.provider.clone(),
                    model: suggestion.model.clone(),
                    request_sha256: suggestion.request_sha256.clone(),
                }
            };
            match field.field.as_str() {
                "name" => {
                    ensure!(
                        !value.is_empty() && value.chars().count() <= 100,
                        "Enter a name up to 100 characters"
                    );
                    self.name = value.clone();
                    self.name_origin = Some(origin);
                }
                "shape" => {
                    self.physics.shape = match value.as_str() {
                        "box" => Shape::Box,
                        "four_leg_table" => Shape::FourLegTable,
                        _ => anyhow::bail!("Use box or four_leg_table"),
                    };
                    self.shape_origin = Some(origin);
                }
                key => {
                    let axis = ["width", "depth", "height"]
                        .iter()
                        .position(|&s| s == key)
                        .context("Unknown property")?;
                    let n: f64 = value.parse().context("Enter a length in metres")?;
                    ensure!(n.is_finite() && n > 0., "Enter a positive length in metres");
                    self.dimensions[axis] = Some(Dimension {
                        value: n,
                        unit: Unit::M,
                        provenance: origin,
                    });
                }
            }
        }
        let field = &mut self.suggestions[proposal].fields[index];
        field.status = decision;
        field.applied = applied;
        Ok(decision)
    }
}
