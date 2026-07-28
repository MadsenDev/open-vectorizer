# Symmetry notes

The mark has strong approximate bilateral symmetry around a vertical axis. The current engine has no explicit mechanism for exploiting that structure.

A future symmetry-aware candidate should be evaluated conservatively:

1. Detect or propose an axis from the coverage field / contour geometry.
2. Measure how closely corresponding geometry agrees under reflection.
3. Only introduce a mirrored/shared candidate when the source itself supports it within tolerance.
4. Rasterize and score the candidate against the original source coverage, exactly like other candidates.
5. Prefer the structurally simpler symmetric representation only when fidelity remains acceptable.

Do not force symmetry onto artwork that is intentionally asymmetric. Symmetry is evidence for a candidate, not ground truth.
