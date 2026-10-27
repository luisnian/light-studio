//! The photos a point has from the phone: downloaded to a folder by the synchronisation, decoded here once and kept as
//! textures while the app is open.

use std::collections::HashMap;

use egui::{Context, TextureHandle, TextureId};

/// The decoded photos by key (`None`: the file is there but is not a picture, so it is not tried again).
#[derive(Default)]
pub struct Cache {
    photos: HashMap<String, Option<(TextureHandle, [usize; 2])>>,
}

impl Cache {
    /// The texture of the photo with this key, if its file has arrived and can be read.
    pub fn get(&mut self, ctx: &Context, key: &str) -> Option<(TextureId, [usize; 2])> {
        if !self.photos.contains_key(key) {
            let dir = crate::pdi::photo_dir()?;
            let file = crate::services::pdi_sync::photo_file(&dir, key)?;
            // not there yet: it may come with the next synchronisation
            let bytes = std::fs::read(file).ok()?;
            let tex = crate::services::pdi_sync::decode_photo(&bytes).map(|(w, h, rgba)| {
                let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                (ctx.load_texture(format!("pdi-photo-{key}"), img, egui::TextureOptions::LINEAR), [w, h])
            });
            self.photos.insert(key.to_string(), tex);
        }
        self.photos.get(key)?.as_ref().map(|(t, size)| (t.id(), *size))
    }
}
