The vendored `yace-stepmania` crate is from YACE commit
`ea87fa3b5057097c2f81512d2f38b3127e0d8fe2` (MIT). It is included so a YARG
checkout builds independently of a sibling YACE repository. See licenses/YACE.txt.
The arrow outline, subdivision palette and noteskin design are adapted from
`src/studio/dance_preview.rs` at the same YACE commit, under the same MIT license.
Its timing implementation includes SMEditor-derived design; see
licenses/SMEditor-MIT.txt. No StepMania artwork or executable code is bundled.

The native ABI is version 1. Input and output strings use UTF-8. Returned strings
must be released with yarg_dance_free_string; sessions with yarg_dance_destroy.
Session functions are called on the gameplay thread; independent parsers/sessions
may run concurrently. Note times use recording seconds, independent of the band
tempo map. Judgment windows are measured in real seconds at the selected speed.
