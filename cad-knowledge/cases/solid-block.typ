#cad_spec("solid-block", parameters: (w: 20mm, d: 12mm, h: 5mm),
  requirements: (require("width", w > 0mm), require("depth", d > 0mm), require("height", h > 0mm)))
#cad_plan("candidate", spec_hash: "6ee02ca225fdffe212ff321104d87237ab17181daff3cdec8155ae35667dc7c4",
  nodes: (outline: rectangle(w, d), body: extrude(outline, h)), output: body)
#cad_target("measurements", spec_hash: "6ee02ca225fdffe212ff321104d87237ab17181daff3cdec8155ae35667dc7c4",
  checks: (
    require_volume("volume", 0.999*w*d*h, 1.001*w*d*h),
    require_span("width", x, w, w),
    require_span("depth", y, d, d),
    require_material("core", 0mm, 0mm, h/2, true),
    require_material("outside", w, 0mm, h/2, false),
  ))
