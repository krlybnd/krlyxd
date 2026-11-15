#cad_spec("solid-cutout", parameters: (w: 20mm, d: 12mm, r: 3mm, h: 5mm),
  requirements: (require("width", w > 2*r), require("depth", d > 2*r), require("height", h > 0mm)))
#cad_plan("candidate", spec_hash: "746ceb89f4b88c9a77cbb5b7a3e143a82499707688ddf2ad29b6f6cff8d8e1a8",
  nodes: (outline: rectangle(w, d), plate: extrude(outline, h), bore: disk(r),
    cutter: extrude(bore, h), body: difference(plate, cutter)), output: body)
#cad_target("measurements", spec_hash: "746ceb89f4b88c9a77cbb5b7a3e143a82499707688ddf2ad29b6f6cff8d8e1a8",
  checks: (
    require_volume("volume", 0.999*(w*d - pi*r*r)*h, 1.001*(w*d - pi*r*r)*h),
    require_span("width", x, w, w),
    require_span("depth", y, d, d),
    require_material("plate", 8mm, 0mm, h/2, true),
    require_material("bore", 0mm, 0mm, h/2, false),
  ))
