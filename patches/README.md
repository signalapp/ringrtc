# WebRTC Patches

The upstream WebRTC repo consists of numerous git sub-repos (not git
submodules, just nested git repos).  In contrast, the stg patch
manager works at the repo level.

The upshot is **every** WebRTC repo that is patched needs its own stg
`series` file.

The directories here mirror the layout of the WebRTC source repo.  If
a WebRTC directory contains a .git directory then the corresponding
directory here will contain a stg `series` file.

## Exporting Patches

Use the `stg export` command to export patches from the working source
tree into the ringrtc source repo.  When doing so , use the template
provided in `stgit-config/patchexport.tmpl` to format the patches.
For example:

    $ cd out/webrtc/src
    $ stg export --template ../../../patches/stgit-config/patchexport.tmpl --dir ../../../patches/webrtc

See stg-export(1) for more details about using templates.
