#cad_spec("solid-ring", parameters: (R: 15mm, r: 6mm, h: 8mm),
  requirements: (require("outer", R > r), require("height", h > 0mm)))
#cad_plan("candidate", spec_hash: "392f2d9d0bf91392578c41f8de9d16c253538e97d361382e56bc5f97ccb35a71",
  nodes: (ring: annulus(R, r), body: extrude(ring, h)), output: body)
#cad_target("measurements", spec_hash: "392f2d9d0bf91392578c41f8de9d16c253538e97d361382e56bc5f97ccb35a71",
  checks: (
    require_volume("volume", 0.999*pi*(R*R - r*r)*h, 1.001*pi*(R*R - r*r)*h),
    require_span("diameter", x, 2*R, 2*R),
    require_span("height", z, h, h),
    require_material("wall", (R+r)/2, 0mm, h/2, true),
    require_material("hole", 0mm, 0mm, h/2, false),
  ))
