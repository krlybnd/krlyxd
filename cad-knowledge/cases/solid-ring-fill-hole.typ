// Review the existing ring into a solid cylinder while preserving its envelope.
#cad_review("fill-hole", model: "solid-ring", base_revision: 0,
  base_hash: "392f2d9d0bf91392578c41f8de9d16c253538e97d361382e56bc5f97ccb35a71",
  geometry_replace: (
    volume: change_geometry(
      require_volume("volume", 0.999*pi*(R*R - r*r)*h, 1.001*pi*(R*R - r*r)*h),
      require_volume("volume", 0.999*pi*R*R*h, 1.001*pi*R*R*h)),
    hole: change_geometry(
      require_material("hole", 0mm, 0mm, h/2, false),
      require_material("hole", 0mm, 0mm, h/2, true)),
  ),
  geometry_preserve: (diameter, height, wall))
