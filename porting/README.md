# porting

The modules written for krly3d before the clean rebuild, kept as the source for porting.
They are not part of the workspace and do not compile: they were written against the
B-rep model and world graph that have been removed.

Delete each file once its replacement under `crates/` passes the same assertions.
Historical notes name a `legacy-opengeometry` branch, but this checkout has no
local or remote-tracking ref for it as of 2026-11-15. Do not rely on that branch
as a recovery source; preserve a local archive before deleting a porting file.
