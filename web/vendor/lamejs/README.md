# MP3 encoder

Rustias uses [lamejs 1.2.1](https://github.com/zhuker/lamejs/tree/260ecf8a2cf15b97e65442986c5c9149b0be7764), by Alex Zhukov, a JavaScript port of [LAME](https://lame.sourceforge.net/), via jump3r. It runs as a separate, replaceable script in the browser export worker. No audio is uploaded.

`lame.all.js` is the **unmodified, readable distribution** from the npm `lamejs@1.2.1` tarball. Its SHA-256 is `026bd88846040f357a937cd85821a48492a362eff0812cda734f23fca55fea3b`. The tarball's SHA-512 integrity was verified before extraction. Full corresponding upstream source and build scripts are available at the pinned revision above and in [its source archive](https://github.com/zhuker/lamejs/archive/260ecf8a2cf15b97e65442986c5c9149b0be7764.tar.gz).

Upstream declares **LGPL-3.0**. `LICENSE` preserves the upstream notice; `COPYING.LESSER` and `COPYING` contain the LGPLv3 and GPLv3 license texts. This dependency retains its own license, independently of Rustias.
