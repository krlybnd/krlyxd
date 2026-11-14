//! `picture path=…`: an image file, read and decoded once per session.

use crate::read::format_of;
use krly3d_core::schema::{Kind, Param, Schema};
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::make::{Make, MakeRequest};
use krlyvis_kernel::ports::read::read;
use krlyvis_kernel::{Context, Datum, Plugin};
use std::sync::Arc;

pub struct Picture;

/// Files larger than this are refused before they are read.
const LARGEST_FILE: u64 = 256 * 1024 * 1024;

impl Plugin<Make> for Picture {
    fn id(&self) -> Id {
        Id::new("picture")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "an image file (.png, .jpg, .pgm, .ppm, .pnm) read once per session; outline reads it",
            inputs: (0, 0),
            params: vec![Param::required("path", Kind::Text, "the image file")],
        })
    }

    fn call(&self, cx: &Context, request: &MakeRequest) -> Attempt<Datum> {
        picture(cx, request).into()
    }
}

fn picture(cx: &Context, request: &MakeRequest) -> Result<Datum, Error> {
    let path = request.params.text("path")?;
    let format = format_of(path).ok_or_else(|| {
        Error::input(format!("{path}: the readers take .png, .jpg, .jpeg, .pgm, .ppm and .pnm files"))
    })?;
    let found = std::fs::metadata(path).map_err(|error| Error::not_found(format!("{path}: {error}")))?;
    if !found.is_file() {
        return Err(Error::input(format!("{path} is not a file")));
    }
    if found.len() > LARGEST_FILE {
        return Err(Error::input(format!(
            "{path} is {} bytes; at most {LARGEST_FILE} are read",
            found.len()
        )));
    }
    let bytes = std::fs::read(path).map_err(|error| Error::not_found(format!("{path}: {error}")))?;
    let image = read(cx, format, bytes).into_result()?;
    Ok(Datum::Picture { path: path.to_string(), image: Arc::new(image) })
}
