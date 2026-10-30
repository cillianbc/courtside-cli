# Third-party software in Courtside's installers

Courtside is licensed under the GNU AGPL v3 ([LICENSE](../LICENSE)). Its installers also carry software by others, under its own licences. Each installer includes a list of that software, the licence texts, and where the source is:

- **macOS:** `Courtside.app/Contents/Resources/third-party/` (right-click the app → Show Package Contents).
- **Windows:** the `third-party` folder in the folder Courtside is installed in.

## Video playback: mpv and FFmpeg (GPL)

Courtside plays video with [libmpv](https://mpv.io), which uses [FFmpeg](https://ffmpeg.org) and other libraries to decode video. These are free software under the GPL (mpv: GPL-2.0-or-later; FFmpeg as built: GPL-3.0-or-later), with LGPL, BSD and MIT parts. Courtside doesn't modify them. Combining them with Courtside is allowed: the AGPL v3 and GPL v3 permit it (section 13 of each).

- **macOS:** the libraries are [Homebrew](https://brew.sh)'s builds, bundled by `tools/bundle/macos.sh`. `THIRD-PARTY-NOTICES.txt` lists each one with its version, licence and source tarball.
- **Windows:** `libmpv-2.dll` is a build from [shinchiro/mpv-winbuild-cmake](https://github.com/shinchiro/mpv-winbuild-cmake), downloaded by the `Desktop app` workflow. `THIRD-PARTY-NOTICES.txt` names the release and the mpv commit; that release's build recipe fixes the version of every library inside.

### Written offer

For three years from the release of any Courtside installer, the Courtside project will provide the complete corresponding source code of the GPL software in that installer, on request, at no more than the cost of providing it. Write to **cillianbc@gmail.com** with the Courtside version and your system.

## Courtside's own dependencies

The Rust crates and JavaScript packages Courtside is built from are under permissive licences (MIT, Apache-2.0, BSD and similar) or the MPL-2.0. They're listed in `Cargo.lock` and `apps/desktop/pnpm-lock.yaml`. The fonts (IBM Plex, Barlow Condensed) are under the SIL Open Font License.

## openvolley

Courtside is built on what the [openvolley](https://openvolley.org) project worked out about DataVolley files, and checks itself against its tools. Thank you to its authors. None of openvolley's code is in the app or its installers; where Courtside follows openvolley, it says so in the code.

| Project | Licence | How Courtside uses it |
|---|---|---|
| [`datavolley`](https://github.com/openvolley/datavolley) (R), © the datavolley package authors (Ben Raymond, Adrien Ickowicz, Tyler Widdison and contributors) | MIT | **The oracle.** `tools/parity` runs it in Docker (`dv_read`, `dv_validate`) and the parity tests compare Courtside's reading of every corpus file with it. **Definitions followed:** the meaning of the `.dvw` columns (`R/meta.R`, `read_main` in `R/plays.R`), the names of a code's extras in the read-back (`R/plays.R`), the volleymetrics and German evaluation meanings (`skill_evaluation_decoder`), court grid positions and zones (`dv_index2xy`), and the plays layout of `courtside convert --to json`. Validator checks modelled on `dv_validate` are marked in `vb-engine/src/validate.rs`. |
| [`fivbvis`](https://github.com/openvolley/fivbvis) (R), © openvolley | MIT | Read alongside FIVB's VIS SDK documentation to build the VIS requests in `crates/vb-vis`. |

openvolley's licence asks that its copyright notice and permission notice go with copies or substantial portions of its software. Courtside copies no openvolley code; the tables it follows (evaluation meanings, column names, grid values) are credited here and at each place in the source. The MIT licence:

```
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
