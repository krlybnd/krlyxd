# porting

The modules written for krly3d before the clean rebuild, kept as the source for porting.
They are not part of the workspace and do not compile: they were written against the
B-rep model and world graph that have been removed.

Delete each file once its replacement under `crates/` passes the same assertions.
The complete pre-rebuild tree is on the `legacy-opengeometry` branch.
