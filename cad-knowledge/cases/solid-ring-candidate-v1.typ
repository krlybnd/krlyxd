// Bind a fresh plan and target to the reviewed revision, h = 10 mm.
#cad_plan("candidate-v1", spec_hash: "6213273ca9ee31362aef7e5b0ccd6a3f3d0048a0294165f82ad48b3ae4e68ed5",
  nodes: (ring: annulus(R, r), body: extrude(ring, h)), output: body)
#cad_target("measurements-v1", spec_hash: "6213273ca9ee31362aef7e5b0ccd6a3f3d0048a0294165f82ad48b3ae4e68ed5",
  checks: (
    require_volume("volume", 0.999*pi*(R*R - r*r)*h, 1.001*pi*(R*R - r*r)*h),
    require_span("diameter", x, 2*R, 2*R),
    require_span("height", z, h, h),
    require_material("wall", (R+r)/2, 0mm, h/2, true),
    require_material("hole", 0mm, 0mm, h/2, false),
  ))
