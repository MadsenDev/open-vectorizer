# Failure notes

Observed current output is substantially worse than desired despite the source being a visually simple logo.

The case appears to combine several known or suspected weaknesses:

1. Thin linework is represented as filled ribbons and therefore traced on both sides rather than recovered as a centreline plus stroke width (#23).
2. Closely spaced anti-aliased curves provide many opportunities for small coverage errors to become visible geometry drift.
3. Strong bilateral symmetry is not currently used as a reconstruction constraint, so mirrored structures can be fitted independently and diverge.
4. The dark-on-dark source may also exercise the low-contrast interior classification weakness tracked in #26.

Treat this as a multi-feature regression case rather than assuming one fix will solve it completely.
