// This is a typed parameter/constraint probe, not a geometric shape proof.
// krlysci cad-contract reads Typst's native numeric-unit syntax here.
#cad_spec("quantity-probe", parameters: (R: 15mm, r: 6mm, h: 8mm),
  requirements: (
    require("positive-height", h > 0mm),
    require("radial-order", R > r),
    require("minimum-gap", R - r >= 2mm),
    require("area-order", R * R > r * r),
  ))
