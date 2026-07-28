# Metrics to add

When the source and baseline output are committed, extend the real-logo harness with measurements for this case.

Suggested metrics:

- raster reconstruction accuracy / coverage error
- total node count
- total shape count
- connected-component preservation for thin linework
- left/right symmetry error after reflection around the detected or declared symmetry axis

Symmetry should be treated as an additional structural signal, not a replacement for raster fidelity. A perfectly mirrored but inaccurate reconstruction is still wrong.
