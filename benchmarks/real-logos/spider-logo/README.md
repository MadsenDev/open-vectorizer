# Spider logo regression case

This case is intended to stress areas that the synthetic benchmark suite does not currently represent well:

- thin curved linework / stroke-like regions
- closely spaced, near-parallel contours
- strong bilateral symmetry and repeated structure
- subtle dark-on-dark / low-contrast boundaries

The current vectorizer performs poorly on this kind of artwork compared with its results on filled geometric marks. It should be used as a regression target while improving stroke recovery, palette handling, and symmetry-aware reconstruction.

## Source image

The source artwork was supplied by the project maintainer during development. Add the raster input here once its redistribution/provenance is recorded explicitly; do not substitute scraped third-party artwork.

## Related issues

- #21 — real-logo benchmark corpus
- #23 — stroke recovery
- #26 — low-contrast palette handling
- symmetry/repetition-aware reconstruction issue

## What to measure

Once the source fixture is committed, record at minimum:

- raster reconstruction accuracy
- node count
- number of output shapes
- whether thin linework remains distinct
- symmetry error between corresponding left/right structures

The goal is not to special-case this logo. It is a representative failure case for thin, symmetric line-art marks.
