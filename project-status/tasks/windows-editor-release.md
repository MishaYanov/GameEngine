# Windows Editor release

Built and verified locally on 2026-11-25 from the main working tree based on cc0e6b9.

The x64 installer and portable Editor are in D:\Builds\editor\Latest. Start Editor on D.cmd carries forward the previous
portable Editor profile on D:. The installer includes the optional Admin Dashboard.

Ran Cargo clean on the dedicated targets and removed generated legacy Cargo profiles, previous Editor packages, and
staging on D:. Initial cleanup reclaimed 67.9 GiB. Kept the Editor user data, game projects, and offline dependencies.

The official Windows release script verified the embedded frontends and the packaged Player, Agent, and Cinder Oath
files. After the build cache was removed, the portable Editor opened its project screen and showed Open Existing Project
and Create. The installer was built without running a system installation.

Build log and portable startup evidence are under target/windows-editor-release-state.
