//! Preview common scene edits against the operator's actual local asset library.
use super::{Instance, Scene};
use crate::survey::asset::Asset;
use anyhow::{bail, ensure, Context, Result};

fn singular(text: &str) -> String {
    let s = text.trim().to_lowercase();
    s.strip_suffix('s').unwrap_or(&s).to_string()
}

fn resolve<'a, T>(query: &str, values: &'a [T], name: impl Fn(&T) -> &str) -> Result<&'a T> {
    let q = singular(query);
    ensure!(!q.is_empty(), "Specify an object name");
    let exact = values
        .iter()
        .filter(|v| singular(name(v)) == q)
        .collect::<Vec<_>>();
    let found = if exact.is_empty() {
        values
            .iter()
            .filter(|v| singular(name(v)).split_whitespace().any(|w| w == q))
            .collect::<Vec<_>>()
    } else {
        exact
    };
    match found.as_slice() {
        [value] => Ok(*value),
        [] => bail!("No library match for ‘{query}’. Choose an asset or enter its full name."),
        _ => bail!(
            "‘{query}’ matches {}. Enter the full name.",
            found.into_iter().map(name).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn instance<'a>(query: &str, items: &'a [Instance]) -> Result<&'a Instance> {
    if let Some((side, kind)) = query
        .split_once(' ')
        .filter(|(s, _)| *s == "left" || *s == "right")
    {
        let mut matches = items
            .iter()
            .filter(|i| {
                i.name
                    .to_lowercase()
                    .split_whitespace()
                    .any(|w| singular(w) == singular(kind))
            })
            .collect::<Vec<_>>();
        ensure!(!matches.is_empty(), "No object matches ‘{kind}’");
        matches.sort_by(|a, b| a.position_m[0].total_cmp(&b.position_m[0]));
        if side == "right" {
            matches.reverse();
        }
        ensure!(
            matches
                .get(1)
                .is_none_or(|b| (matches[0].position_m[0] - b.position_m[0]).abs() > 1e-6),
            "Two objects share that position. Enter an instance name."
        );
        Ok(matches[0])
    } else {
        resolve(query, items, |v| v.name.as_str())
    }
}

impl Scene {
    /// The caller reviews the returned layout before replacing the current draft.
    pub fn propose(&self, instruction: &str, library: &[Asset]) -> Result<Scene> {
        let command = instruction
            .trim()
            .trim_end_matches(['.', '!'])
            .to_lowercase();
        let mut next = self.clone();
        let article = |s: &str| s.strip_prefix("the ").unwrap_or(s).to_string();
        if let Some(request) = command.strip_prefix("move ") {
            let (from, to) = request
                .split_once(" beside ")
                .context("Use ‘move [object] beside [object]’")?;
            let a = instance(&article(from), &self.instances)?;
            let b = instance(&article(to), &self.instances)?;
            ensure!(a.id != b.id, "Choose two different instances");
            let extent = |item: &Instance| -> Result<f64> {
                let asset = library
                    .iter()
                    .find(|x| x.id == item.asset.id && x.revision == item.asset.revision)
                    .context("The pinned asset is missing")?;
                let (s, c) = item.yaw_deg.to_radians().sin_cos();
                let w = asset.dimensions[0]
                    .as_ref()
                    .context("Measure object width before placing it beside another")?
                    .metres();
                let d = asset.dimensions[1]
                    .as_ref()
                    .context("Measure object depth before placing it beside another")?
                    .metres();
                Ok((c.abs() * w + s.abs() * d) * 0.5)
            };
            let offset = extent(a)? + extent(b)? + 0.10;
            let side = if b.position_m[0] + offset <= self.room_m[0] / 2. - extent(a)? {
                1.
            } else {
                -1.
            };
            let item = next.instances.iter_mut().find(|i| i.id == a.id).unwrap();
            item.position_m = [b.position_m[0] + side * offset, b.position_m[1], 0.];
            return Ok(next);
        }
        let list = command
            .strip_prefix("place ")
            .or_else(|| command.strip_prefix("add "))
            .unwrap_or(&command);
        let list = list
            .strip_suffix(" in the room")
            .unwrap_or(list)
            .replace(", and ", " and ")
            .replace(", ", " and ");
        for clause in list.split(" and ") {
            let (first, rest) = clause.trim().split_once(' ').unwrap_or((clause, ""));
            let count = match first {
                "a" | "an" | "one" => Some(1),
                "two" => Some(2),
                "three" => Some(3),
                "four" => Some(4),
                v => v.parse::<usize>().ok(),
            };
            let query = if count.is_some() { rest } else { clause };
            let count = count.unwrap_or(1);
            ensure!(
                count > 0 && next.instances.len().saturating_add(count) <= 128,
                "Choose between 1 and 128 instances"
            );
            let asset = resolve(query, library, |a| a.name.as_str())?;
            for _ in 0..count {
                let n = next.instances.len();
                let cols = (self.room_m[0] / 1.2).floor().clamp(1., 12.) as usize;
                let rows = (self.room_m[1] / 1.2).floor().clamp(1., 12.) as usize;
                let x = ((n % cols) as f64 + 0.5) / cols as f64 - 0.5;
                let y = (((n / cols) % rows) as f64 + 0.5) / rows as f64 - 0.5;
                next.place(asset, [x * self.room_m[0], y * self.room_m[1], 0.])?;
            }
        }
        Ok(next)
    }
}
