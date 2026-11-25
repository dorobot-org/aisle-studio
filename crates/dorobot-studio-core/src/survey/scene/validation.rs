use super::{Instance, Revision, Scene};
use crate::survey::asset::{Asset, Collider, Surface};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Support {
    pub instance: String,
    pub surface: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub instance: String,
    pub message: String,
}

struct Bounds {
    center: [f64; 3],
    half: [f64; 3],
    axes: [[f64; 2]; 2],
}

fn axes(yaw: f64) -> [[f64; 2]; 2] {
    let (s, c) = yaw.to_radians().sin_cos();
    [[c, s], [-s, c]]
}
fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn world(object: &Instance, local: [f64; 3]) -> [f64; 3] {
    let [a, b] = axes(object.yaw_deg);
    [
        object.position_m[0] + local[0] * a[0] + local[1] * b[0],
        object.position_m[1] + local[0] * a[1] + local[1] * b[1],
        object.position_m[2] + local[2],
    ]
}
impl Bounds {
    fn new(item: &Instance, collider: &Collider) -> Self {
        Self {
            center: world(item, collider.center_m),
            half: collider.size_m.map(|v| v / 2.),
            axes: axes(item.yaw_deg),
        }
    }
    fn extent(&self, dir: [f64; 2]) -> f64 {
        dot(self.axes[0], dir).abs() * self.half[0] + dot(self.axes[1], dir).abs() * self.half[1]
    }
    fn intersects(&self, other: &Self) -> bool {
        const EPS: f64 = 1e-7;
        if (self.center[2] - other.center[2]).abs() >= self.half[2] + other.half[2] - EPS {
            return false;
        }
        let delta = [
            self.center[0] - other.center[0],
            self.center[1] - other.center[1],
        ];
        self.axes
            .iter()
            .chain(&other.axes)
            .all(|&dir| dot(delta, dir).abs() < self.extent(dir) + other.extent(dir) - EPS)
    }
}

impl Scene {
    fn support<'a>(
        &'a self,
        target: &Support,
        assets: &'a HashMap<Revision, Asset>,
    ) -> Result<(&'a Instance, &'a Surface)> {
        let object = self
            .instances
            .iter()
            .find(|v| v.id == target.instance)
            .context("The support object is missing")?;
        let surface = assets
            .get(&object.asset)
            .and_then(|a| {
                a.geometry
                    .surfaces
                    .iter()
                    .find(|s| s.name == target.surface)
            })
            .context("The named placement surface is missing")?;
        Ok((object, surface))
    }
    pub fn snap_to(
        &mut self,
        id: &str,
        target: Option<Support>,
        assets: &HashMap<Revision, Asset>,
    ) -> Result<()> {
        let center = if let Some(link) = &target {
            ensure!(id != link.instance, "An object cannot support itself");
            let (object, surface) = self.support(link, assets)?;
            let mut parent = Some(object);
            let mut seen = std::collections::HashSet::new();
            while let Some(p) = parent {
                ensure!(p.id != id && seen.insert(&p.id), "Cyclic object support");
                parent = p
                    .support
                    .as_ref()
                    .and_then(|s| self.instances.iter().find(|i| i.id == s.instance));
            }
            Some(world(object, surface.center_m))
        } else {
            None
        };
        let object = self
            .instances
            .iter_mut()
            .find(|i| i.id == id)
            .context("Select an object")?;
        if let Some(p) = center {
            object.position_m = p;
        } else {
            object.position_m[2] = 0.;
        }
        object.support = target;
        Ok(())
    }

    pub fn issues(&self, assets: &HashMap<Revision, Asset>) -> Vec<Issue> {
        let mut issues = vec![];
        let mut placed: Vec<(&Instance, Vec<Bounds>)> = vec![];
        for item in &self.instances {
            let mut issue = |message: String| {
                issues.push(Issue {
                    instance: item.id.clone(),
                    message,
                })
            };
            let Some(asset) = assets.get(&item.asset) else {
                issue("Pinned asset is missing".into());
                continue;
            };
            if asset.geometry.colliders.is_empty() {
                issue("Measure this asset to generate its collision geometry".into());
            }
            if item.position_m[2] < -1e-7 {
                issue("Object intersects the floor".into());
            }
            let bounds = asset
                .geometry
                .colliders
                .iter()
                .map(|body| Bounds::new(item, body))
                .collect::<Vec<_>>();
            if bounds.iter().any(|b| {
                b.center[0].abs() + b.extent([1., 0.]) > self.room_m[0] / 2. + 1e-7
                    || b.center[1].abs() + b.extent([0., 1.]) > self.room_m[1] / 2. + 1e-7
            }) {
                issue("Collider extends outside the room".into());
            }
            if let Some(target) = &item.support {
                match self.support(target, assets) {
                    Err(e) => issue(e.to_string()),
                    Ok((parent, surface)) => {
                        let center = world(parent, surface.center_m);
                        let dirs = axes(parent.yaw_deg);
                        let outside = bounds.iter().any(|b| {
                            let delta = [b.center[0] - center[0], b.center[1] - center[1]];
                            (0..2).any(|axis| {
                                dot(delta, dirs[axis]).abs() + b.extent(dirs[axis])
                                    > surface.size_m[axis] / 2. + 1e-7
                            })
                        });
                        if outside || (center[2] - item.position_m[2]).abs() > 1e-6 {
                            issue(format!(
                                "Object is outside {} on {}",
                                surface.name, parent.name
                            ));
                        }
                    }
                }
            }
            for (other, boxes) in &placed {
                if bounds.iter().any(|a| boxes.iter().any(|b| a.intersects(b))) {
                    issue(format!("Collider intersects {}", other.name));
                }
            }
            placed.push((item, bounds));
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::survey::asset::{Dimension, Origin, Shape, Unit};

    #[test]
    fn checks_actual_leg_clearance_rotated_bounds_and_bound_support() -> Result<()> {
        let mut table = Asset {
            id: "asset-table".into(),
            revision: 1,
            name: "Table".into(),
            ..Default::default()
        };
        table.physics.shape = Shape::FourLegTable;
        let dimension = |m| {
            Some(Dimension {
                value: m,
                unit: Unit::M,
                provenance: Origin::Operator,
            })
        };
        table.dimensions = [dimension(0.8), dimension(0.5), dimension(0.45)];
        table.geometry = table.calculate_geometry()?;
        let mut cube = Asset {
            id: "asset-cube".into(),
            revision: 1,
            name: "Cube".into(),
            ..Default::default()
        };
        cube.dimensions = [dimension(0.06), dimension(0.06), dimension(0.06)];
        cube.geometry = cube.calculate_geometry()?;
        let mut scene = Scene::default();
        let stand = scene.place(&table, [0., 0., 0.])?;
        let item = scene.place(&cube, [0., 0., 0.])?;
        let assets = HashMap::from([
            (scene.instances[0].asset.clone(), table),
            (scene.instances[1].asset.clone(), cube),
        ]);
        assert!(
            scene.issues(&assets).is_empty(),
            "Clear space under the tabletop"
        );
        scene.snap_to(
            &item,
            Some(Support {
                instance: stand.clone(),
                surface: "tabletop".into(),
            }),
            &assets,
        )?;
        assert_eq!(scene.instances[1].position_m, [0., 0., 0.45]);
        assert!(
            scene.issues(&assets).is_empty(),
            "Contact at the support plane"
        );
        scene.instances[1].position_m[0] = 0.39;
        assert!(scene
            .issues(&assets)
            .iter()
            .any(|i| i.instance == item && i.message.contains("outside tabletop")));
        scene.instances[0].yaw_deg = 90.;
        scene.snap_to(&item, None, &assets)?;
        scene.instances[1].position_m = [0.2275, 0.3775, 0.];
        assert!(scene
            .issues(&assets)
            .iter()
            .any(|i| i.instance == item && i.message.contains("intersects Table")));
        scene.instances[1].position_m = [5., 0., -1.];
        assert_eq!(
            scene
                .issues(&assets)
                .iter()
                .filter(|i| i.instance == item)
                .count(),
            2
        );
        Ok(())
    }
}
