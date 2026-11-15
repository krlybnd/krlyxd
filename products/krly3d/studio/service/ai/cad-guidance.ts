/** Small curated modeling-method retrieval. These are process references, never
 * dimensional evidence, a product model, executable code or certification.
 * Public tutorials are distilled once; each build need not pay to rediscover CAD.
 */
export function cadGuidance(text: string) {
  const basic = {
    title: "FreeCAD: sketch-based Part Design workflow",
    url:
      "https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/Basic_Part_Design_Tutorial.md",
    method:
      "Choose a datum and draw the dimensioned base sketch with lines/arcs; extrude material thickness. Use separate sketches for pockets/cuts. Apply supported edge finishing last. Preserve each operation and its input sketch in the feature history. Check dimensions after construction. Explicit coordinates are not a solved constraint system.",
  };
  if (
    /vill[aá]s\s*kulcs|vill[aá]skulcs|open[- ]end(?:ed)?\s+(?:wrench|spanner)|\bwrench\b|\bspanner\b/iu
      .test(text)
  ) {
    return [basic, {
      title: "SolidWorks Instructor's Guide (2010), Lesson 3: wrench",
      url:
        "https://www.solidworks.com/sw/docs/Instructor_WB_2011_ENG.pdf#page=73",
      method:
        "The wrench tutorial builds a rounded handle and wider head, cuts a slot with a rounded closed end for the open jaw, then finishes selected edges. Adapt the method to supported operations: a base sketch may combine handle/head outline, then extrude; a separate open-slot pocket must cross the head boundary, preserving the specified gap between parallel jaw faces. Optional handle recess is another shallow pocket. A sketch arc is a true circular arc, not a faceted imitation. This tutorial supplies a modeling method, NOT dimensions for the requested size or a manufacturer's exact shape.",
    }];
  }
  return [basic];
}
