# Tag Decompositions

This directory contains mappings of serialization tags into their child tags.
It is used for automated testing to ensure that the format of any given tag is
not changed accidentally, and that version numbers are bumped when required.
For this purpose, old versions are kept around.

If you get an error concerning this, you should first consider incrementing the
version number in the tag of the type causing the error -- only if you know
that this change *is* backwards-compatible (for instance, if the tag
decomposition representation has changed, but the underlying structure is the
same, or if an enum variant is added to an enum which is non-exhaustive by
design), OR if this version has not yet been included in a release (pre-release
of otherwise), may files here be overwritten.
