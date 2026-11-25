//! Project-owned survey exposures. Each complete import is published by one directory rename.
use anyhow::{bail, Context, Result};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    pub id: String,
    pub filename: String,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
}

pub struct Survey {
    root: PathBuf,
}

impl Survey {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn photos(&self) -> PathBuf {
        self.root.join("photos")
    }

    pub fn list(&self) -> Result<Vec<Photo>> {
        let root = self.photos();
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut photos = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with('.') || !entry.file_type()?.is_dir()
            {
                continue;
            }
            let photo: Photo = serde_json::from_slice(&fs::read(entry.path().join("photo.json"))?)?;
            if entry.file_name().to_str() != Some(&photo.id) {
                bail!("Survey photo record has the wrong ID");
            }
            photos.push(photo);
        }
        photos.sort_by(|a, b| (&a.filename, &a.id).cmp(&(&b.filename, &b.id)));
        Ok(photos)
    }

    pub fn import(&self, path: &Path) -> Result<Photo> {
        let filename = path
            .file_name()
            .context("Select a photo file")?
            .to_string_lossy()
            .into_owned();
        let bytes = fs::read(path).with_context(|| format!("Read {filename}"))?;
        let format = image::guess_format(&bytes).context("Select a JPEG or PNG photo")?;
        let ext = match format {
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Png => "png",
            _ => bail!("Select a JPEG or PNG photo"),
        };
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let mut identity = Sha256::new();
        identity.update(sha256.as_bytes());
        identity.update(filename.as_bytes());
        let id = format!("{:x}", identity.finalize());
        let root = self.photos();
        fs::create_dir_all(&root)?;
        let dest = root.join(&id);
        let reopen = || -> Result<Photo> {
            let photo: Photo = serde_json::from_slice(&fs::read(dest.join("photo.json"))?)?;
            if photo.sha256 != sha256 || photo.id != id {
                bail!("Stored survey photo identity differs");
            }
            Ok(photo)
        };
        if dest.exists() {
            return reopen();
        }
        let mut decoder = ImageReader::with_format(Cursor::new(&bytes), format).into_decoder()?;
        let orientation = decoder.orientation()?;
        let mut image = DynamicImage::from_decoder(decoder)?;
        image.apply_orientation(orientation);
        let photo = Photo {
            id: id.clone(),
            filename,
            format: ext.into(),
            width: image.width(),
            height: image.height(),
            sha256: sha256.clone(),
        };
        let staging = tempfile::tempdir_in(&root)?;
        fs::write(staging.path().join(format!("original.{ext}")), &bytes)?;
        image
            .thumbnail(1200.min(photo.width), 900.min(photo.height))
            .to_rgb8()
            .save(staging.path().join("preview.png"))?;
        image
            .thumbnail(144.min(photo.width), 96.min(photo.height))
            .to_rgb8()
            .save(staging.path().join("thumbnail.png"))?;
        fs::write(
            staging.path().join("photo.json"),
            serde_json::to_vec_pretty(&photo)?,
        )?;
        match fs::rename(staging.path(), &dest) {
            Ok(()) => Ok(photo),
            Err(_) if dest.is_dir() => reopen(),
            Err(err) => Err(err.into()),
        }
    }

    pub fn room(&self, photo: &Photo) -> Result<String> {
        let path = self.photos().join(&photo.id).join("room.json");
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(err) => Err(err.into()),
        }
    }

    pub fn assign_room(&self, photo: &Photo, name: &str) -> Result<()> {
        let room = name.trim();
        if room.chars().any(char::is_control) {
            bail!("Use a single-line room name");
        }
        let folder = self.photos().join(&photo.id);
        let mut tmp = tempfile::NamedTempFile::new_in(&folder)?;
        tmp.write_all(&serde_json::to_vec(room)?)?;
        tmp.as_file().sync_all()?;
        tmp.persist(folder.join("room.json"))?;
        Ok(())
    }

    pub fn preview(&self, photo: &Photo, thumbnail: bool) -> Result<image::RgbImage> {
        let name = if thumbnail {
            "thumbnail.png"
        } else {
            "preview.png"
        };
        Ok(image::open(self.photos().join(&photo.id).join(name))?.to_rgb8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn originals_and_previews_survive_source_relocation_and_duplicate_imports() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let project = dir.path().join("project");
        let store = Survey::new(project.clone());
        for (ext, format) in [("png", ImageFormat::Png), ("jpg", ImageFormat::Jpeg)] {
            let source = dir.path().join(format!("table.{ext}"));
            image::RgbImage::from_pixel(64, 32, image::Rgb([25, 40, 61]))
                .save_with_format(&source, format)?;
            let p = store.import(&source)?;
            assert_eq!(p, store.import(&source)?);
            fs::rename(&source, source.with_extension("moved"))?;
            assert_eq!((p.width, p.height), (64, 32));
            assert_eq!(
                p.sha256,
                format!(
                    "{:x}",
                    Sha256::digest(fs::read(
                        store.photos().join(&p.id).join(format!("original.{ext}"))
                    )?)
                )
            );
            assert_eq!(store.preview(&p, false)?.width(), 64);
        }
        let invalid = dir.path().join("broken.jpg");
        fs::write(&invalid, b"corrupt")?;
        assert!(store.import(&invalid).is_err());
        assert_eq!(Survey::new(project).list()?.len(), 2);
        Ok(())
    }
}

pub mod asset;
pub use image::RgbImage as PhotoPixels;
