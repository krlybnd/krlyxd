#cad_spec("plus-composition", parameters: (long: 20mm, short: 5mm, h: 4mm),
  requirements: (
    require("width_order", long > short),
    require("height_positive", h > 0mm),
  ))
#cad_target("measured-shape", spec_hash: "1bdeb3356386779791af59018b3d2efa3e87be93d6d4ff2606d98ab57654a6ad",
  checks: (
    require_volume("volume", 0.999*(2*long*short - short*short)*h,
      1.001*(2*long*short - short*short)*h),
    require_span("x-span", x, long, long),
    require_span("y-span", y, long, long),
    require_span("z-span", z, h, h),
    require_material("horizontal-arm", 9mm, 0mm, h/2, true),
    require_material("vertical-arm", 0mm, 9mm, h/2, true),
    require_material("corner-empty", 9mm, 9mm, h/2, false),
    require_material("center", 0mm, 0mm, h/2, true),
  ))
