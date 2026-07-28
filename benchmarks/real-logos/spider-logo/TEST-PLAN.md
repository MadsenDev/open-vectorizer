# Test plan

When automated:

1. Vectorize with the documented default/logo settings.
2. Rasterize the SVG through the benchmark renderer.
3. Compare coverage/error against the source.
4. Record shape and node complexity.
5. Verify thin components have not merged unexpectedly.
6. If symmetry metrics are available, reflect geometry around the fitted vertical axis and measure disagreement.

The case should initially be non-blocking/known-failing so it can be committed without pretending the current engine already handles it well. Tighten thresholds as the relevant issues land.
