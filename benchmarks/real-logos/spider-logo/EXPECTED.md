# Expected behaviour

This fixture is currently a known failure case.

A successful future result should preserve the visual structure of the mark without exploding thin strokes into unnecessarily complex paired contours.

In particular:

- corresponding left/right structures should remain visually symmetric
- thin curved lines should remain separate where the raster indicates separation
- recovered curves should be smooth rather than wobbling along anti-aliased boundaries
- repeated mirrored structures should not independently drift into visibly different geometry
- improvements should still pass raster reconstruction checks rather than preferring symmetry by fiat

Quantitative thresholds will be added when the source raster and expected SVG are committed.
