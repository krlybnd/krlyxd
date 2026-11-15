#cad_spec("solid-cylinder", parameters: (r: 4mm, h: 30mm),
  requirements: (require("radius", r > 0mm), require("height", h > 0mm)))
#cad_plan("candidate", spec_hash: "fae0a2d178cf33ea3b16cb4a0da40f6b9d67a9d3c9a6ffe3afa550f827c59a9d",
  nodes: (outline: disk(r), body: extrude(outline, h)), output: body)
#cad_target("measurements", spec_hash: "fae0a2d178cf33ea3b16cb4a0da40f6b9d67a9d3c9a6ffe3afa550f827c59a9d",
  checks: (
    require_volume("volume", 0.999*pi*r*r*h, 1.001*pi*r*r*h),
    require_span("diameter", x, 2*r, 2*r),
    require_span("height", z, h, h),
    require_material("axis", 0mm, 0mm, h/2, true),
    require_material("outside", 2*r, 0mm, h/2, false),
  ))
