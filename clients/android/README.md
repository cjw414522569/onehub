# Android client boundary

- Layer: L5 (native UI shell)
- Approved bridge: `bindings/kotlin`
- Status: `interface-only`; Kotlin/Gradle/Compose toolchains are intentionally not claimed as built in the Windows-first phase.
- The boundary is versioned in `contract.json` and must not link concrete SSH or persistence modules.

