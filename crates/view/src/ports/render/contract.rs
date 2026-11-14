//! What every renderer must satisfy: the image has the size asked for and names only
//! faces that exist; a pixel shows the face its line of sight meets first; every pixel
//! that shows nothing is painted alike; and asking again gives the same image.

use super::{render, Image, RenderRequest, CHANNELS};
use krly3d_core::Attempt;
use krly3d_kernel::ports::probe::cast;
use krly3d_kernel::Context;

/// Lines of sight checked along the width and along the height of the image.
const SAMPLES: usize = 16;

/// Renders the request with the renderer registered as `renderer`, checks the image and
/// hands it over.
pub fn check(cx: &Context, renderer: &str, request: &RenderRequest) -> Result<Image, String> {
    let image = match (render(cx, renderer, request), render(cx, renderer, request)) {
        (Attempt::Done(first), Attempt::Done(second)) if first == second => first,
        (Attempt::Done(_), Attempt::Done(_)) => {
            return Err(format!("`{renderer}` draws a different image when asked again"))
        }
        (Attempt::Done(_), unanswered) | (unanswered, _) => {
            return Err(format!("`{renderer}` drew nothing: {:?}", unanswered.map(|_| ())))
        }
    };
    let (width, height) = request.size;
    if image.projection.size != request.size
        || image.seen.len() != width * height
        || image.pixels.len() != width * height * CHANNELS
    {
        return Err(format!("the image of `{renderer}` is not {width} by {height} pixels"));
    }
    let known =
        |body: usize, face: usize| request.bodies.get(body).is_some_and(|body| face < body.faces.len());
    if let Some(stray) = image.seen.iter().flatten().find(|seen| !known(seen.body, seen.face)) {
        return Err(format!("a pixel shows face {} of body {}, which is not there", stray.face, stray.body));
    }
    let mut empty = image.seen.iter().zip(image.pixels.chunks(CHANNELS)).filter(|(seen, _)| seen.is_none());
    if let Some((_, background)) = empty.next() {
        if empty.any(|(_, colour)| colour != background) {
            return Err("pixels that show nothing are not all painted alike".into());
        }
    }
    for row in (0..height).step_by(height.div_ceil(SAMPLES)) {
        for column in (0..width).step_by(width.div_ceil(SAMPLES)) {
            check_pixel(cx, request, &image, column, row)?;
        }
    }
    Ok(image)
}

/// Compares what one pixel shows with what its line of sight meets first. Where faces
/// coincide, or the line runs through an edge, any of the faces met there will do.
fn check_pixel(
    cx: &Context,
    request: &RenderRequest,
    image: &Image,
    column: usize,
    row: usize,
) -> Result<(), String> {
    let ray = image.projection.ray(column, row);
    let mut met = Vec::new();
    for (body, data) in request.bodies.iter().enumerate() {
        match cast(cx, data, vec![ray]) {
            Attempt::Done(per_ray) => {
                met.extend(per_ray.into_iter().flatten().filter(|hit| hit.t > 0.0).map(|hit| (body, hit)))
            }
            // What no probe can decide, no renderer can be held to.
            Attempt::Declined(_) => return Ok(()),
            Attempt::Failed(error) => return Err(format!("pixel {column},{row}: {error}")),
        }
    }
    let nearest = met.iter().map(|(_, hit)| hit.t).fold(f64::INFINITY, f64::min);
    let first: Vec<(usize, usize)> = met
        .iter()
        .filter(|(_, hit)| cx.accuracy().is_zero_length(hit.t - nearest))
        .map(|(body, hit)| (*body, hit.face))
        .collect();
    let shown = image.seen[row * image.width() + column].map(|seen| (seen.body, seen.face));
    let agrees = match shown {
        Some(shown) => first.contains(&shown),
        None => first.is_empty(),
    };
    if agrees {
        Ok(())
    } else {
        Err(format!("pixel {column},{row} shows {shown:?}, its line of sight meets {first:?} first"))
    }
}
