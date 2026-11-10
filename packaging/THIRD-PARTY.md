# Third-party software shipped with aiterm

## busybox-w32 (`busybox.exe`)

aiterm for Windows ships `busybox.exe` as its built-in shell, used when Git for Windows is not installed. It runs as a separate program; aiterm does not link it.

- **What:** busybox-w32, the Windows port of BusyBox, 64-bit Unicode build (`busybox-w64u`), renamed to `busybox.exe`
- **Version:** `FRP-6354-g5f09fb446` (BusyBox v1.39.0)
- **Binary:** https://frippery.org/files/busybox/busybox-w64u-FRP-6354-g5f09fb446.exe
  (SHA-256 `36b266923fc5765b42f5f981319f39568498fba60b53ccf26e8fc1e575e2a9a9`)
- **Corresponding source:** https://frippery.org/files/busybox/busybox-w32-FRP-6354-g5f09fb446.tgz,
  also tag `FRP-6354-g5f09fb446` of https://github.com/rmyorston/busybox-w32
- **Project:** https://frippery.org/busybox/ (Ron Yorston and the BusyBox authors)
- **Licence:** GNU General Public License, version 2 (GPL-2.0-only). Full text: https://www.gnu.org/licenses/old-licenses/gpl-2.0.txt

aiterm's own code is under the MIT licence (see `LICENSE`); the GPL applies to `busybox.exe` only. You may get the source above, and redistribute busybox.exe, under the terms of the GPL-2.0. The same source tarball is attached to every aiterm release on GitHub.

## xterm.js (inside `aiterm-app.exe`)

aiterm's window draws the terminal with xterm.js and its fit and WebGL addons, compiled into `aiterm-app.exe`.

- **Project:** https://github.com/xtermjs/xterm.js
- **Licence:** MIT, notices below

### xterm.js

```
Copyright (c) 2017-2019, The xterm.js authors (https://github.com/xtermjs/xterm.js)
Copyright (c) 2014-2016, SourceLair Private Company (https://www.sourcelair.com)
Copyright (c) 2012-2013, Christopher Jeffrey (https://github.com/chjj/)
```

### @xterm/addon-fit

```
Copyright (c) 2019, The xterm.js authors (https://github.com/xtermjs/xterm.js)
```

### @xterm/addon-webgl

```
Copyright (c) 2018, The xterm.js authors (https://github.com/xtermjs/xterm.js)
```

All three are under these terms:

```
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```
