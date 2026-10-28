# Spike Q18: how PrusaSlicer and Bambu Studio import a multi-part 3MF

**Question** (new; related to [13, Q06](../plan/13-risks-and-open-questions.md#2-open-questions-with-spikes) and [05 section 7](../plan/05-3d-printing-pipeline.md#7-colour-and-multi-material-in-3mf)): the owner prints two-colour logo tiles on a Bambu Lab A1 with an AMS lite (Bambu Studio) and on a Prusa XL with several toolheads (PrusaSlicer). When either slicer opens a plain 3MF Core file from OpenBrick3D, the body and the logo should arrive as two parts of one object, in place, so the user only gives each part a filament. How do PrusaSlicer 2.9.x and Bambu Studio 2.x import (a) several build items, (b) one build item whose object has `<components>` that reference mesh objects, and (c) colour groups of the Materials extension? Which layout loads as one object with its parts in place? Does either slicer map colour groups to filaments or extruders?

**Answer: write one components object with one build item (layout b).** Bambu Studio turns an object with components into one object with one part per component, each keeping its transform. PrusaSlicer splits components into separate objects, exactly as it handles several build items, so for PrusaSlicer the two layouts load the same way. Because the parts' boxes touch, PrusaSlicer then asks "Multi-part object detected". Yes merges them into one object with parts in place and gives the parts extruders 1, 2, … in object order. With several build items (layout a), Bambu Studio loads separate objects, and users report that it drops each one onto the bed, so an embossed logo would leave the face of its tile. The 3MF Core specification supports this choice: consumers may rearrange build items, but they must respect the relative positions of components. Colour groups are not a reliable way to assign filaments. PrusaSlicer ignores them. Bambu Studio 2.5 and later reads them, but turns them into surface painting through a "Standard 3MF Import color" dialog, and that has open bugs. `openbrick_io` therefore gains `ExportLayout::OneObjectWithParts`, which is `ExportLayout::PART_DESIGNER`, the layout for part-designer exports. `write_3mf` keeps the old layout, so existing files and fixtures stay byte-identical.

**Date:** 2026-10-28. **Slicers:**
- PrusaSlicer 2.9.6, the latest stable release (25 June 2026). Its source was read at tag `version_2.9.6`, except `Plater.cpp`, which was read at `version_2.9.2`.
- Bambu Studio 2.8.2.61, the latest public release. Its source was read from `master` on 2026-10-28, not from a release tag.

**Method:** public source code, release notes, issue trackers and forum threads, read on the web. No slicer binary was downloaded or run. The source files were read through a fetch tool that summarises long files and quotes short lines; the quoted fragments below came back verbatim. PrusaSlicer, Bambu Studio and OrcaSlicer are AGPL, and no code from them is used here: this report describes their behaviour only. Each claim is marked:

- **source**: read in the slicer's source;
- **reported**: release notes, an issue or a forum thread;
- **unconfirmed**: an inference, or a reading that could not be finished.

None of them is a test on a real slicer. That needs the print test at the end of this report.

## PrusaSlicer 2.9.x

**(b) Components are split into objects (source).** In `src/libslic3r/Format/3mf.cpp`, an `<object>` without a mesh is not made into a model object. It is deleted, and its component list is stored as an alias. A build item that names such an object is resolved recursively by `_create_object_instance`, under the comment `// recursively process nested components`. The recursion passes `transform * component.transform` and adds one instance to each referenced mesh object. So each component mesh becomes its own object, placed by the item transform times the component transform. Nothing becomes a volume (a "part") at this stage. The recursion is capped at 10 levels.

**(a) Several build items give one object each (source).** Each `<item>` adds an instance to the object it names. When the file was not written by PrusaSlicer (no `slic3rpe:Version3mf` metadata, so `m_version == 0`), each object is loaded as a single volume. An object with exactly one instance has its instance transform baked into its mesh (`_generate_volumes`, read at 2.9.2).

**Both layouts then reach the same multi-part check (source).** `FileReader::load_model` computes `looks_like_multipart_object` for every file type, 3MF included; only the unit checks are skipped for 3MF.
- The check is true when there is more than one object, every object has exactly one volume and at most one config key, and some object's box `intersects` or `shares_boundary` with the first object's box (read at 2.9.6).
- An embossed logo shares the face plane with the body's box. An inlaid logo fill lies inside it. Both pass.

`Plater::priv::load_files` asks only for files that are not projects. A 3MF is a project only if it carries PrusaSlicer configuration, so OpenBrick3D's files are asked about. The dialog:
- title: "Multi-part object detected";
- text: "This file contains several objects positioned at multiple heights." … "the file be loaded as a single object having multiple parts?";
- an "Apply to all objects being loaded" checkbox.

**Yes keeps the parts in place and assigns extruders (source).** `ModelProcessing::convert_to_multipart_object(model, nozzle_count)` merges every object into one object.
- Each volume keeps instance transform × volume transform, "to a common reference system (world)".
- Each volume gets `config.set("extruder", auto_extruder_id(...))`, which counts 1, 2, …, wrapping at the number of nozzles in the printer profile. So the first object in the file gets extruder 1 and the second gets extruder 2. On a two-toolhead XL that means body = T1 and logo = T2.
- **Unconfirmed:** that the object order is the order of the mesh objects in the file. Model objects are created as the importer parses them; the code was not traced that far.
- Each part keeps its source object's name. The intended `_2`, `_3` suffixes never appear, because of how the counter is written. So the `ExportObject` names are what the user sees.
- **Unconfirmed:** what the merged object is named in this path. In the comparable dialog for several files, it is named after the first file.

**No silently keeps separate objects (reported).** Prusa forum threads describe answering No to keep separate objects. Prusa's help article on multi-material imports describes the same Yes prompt for several STL files ("PrusaSlicer will ask you if it should represent the loaded models as one single object with multiple parts"; seen as a search-result excerpt, the page itself was not opened).

**(c) Colour groups are ignored (source).** The importer has no handler for `m:colorgroup` or `m:basematerials`. The triangle handler says "we are ignoring the following attributes: p1 p2 p3 pid". The only per-triangle colour data it reads is its own `slic3rpe:mmu_segmentation`, with a fallback to Bambu's `paint_color`. Writing a colour group therefore costs nothing in PrusaSlicer and gains nothing.

**PrusaSlicer 3.0** is in alpha (alpha12, 21 September 2026). Its notes say "when importing into an already-open project, only geometry is imported now" (reported). It was not studied further.

## Bambu Studio 2.x

**(b) Components become parts of one object (source, `master`).** In `src/libslic3r/Format/bbs_3mf.cpp`, a build item's object is collected with `_generate_current_object_list`. Without Bambu metadata, one volume is added per component (`volumes.emplace_back(object_id.second)`). `_generate_volumes_new` then sets each volume's transform to the component transform (`volume->set_transformation(comp_transformatino * volume->get_transformation())`). So the components object loads as one object whose parts keep their places. Bambu Studio's own project files are built the same way: one object per build item, its parts as components. **Unconfirmed:**
- that mesh objects referenced only as components get no object of their own. Model objects are created from build items, and `_handle_end_model` "deletes all non-built or non-instanced objects", but the code between was not fully visible;
- what the parts are called in the object list.

A file counts as Bambu's own when its `Application` metadata starts with `BambuStudio-`; OpenBrick3D's does not.

**(a) Several build items load as separate objects (source and reported).** Each build item creates an object, and objects with more than one instance are split ("split the object in as many objects as instances"). Users report that a Blender 3MF of three stacked objects loaded as three objects "compressed down to the floor" (March 2026). Another user reports that each imported 3MF is centred with its lowest point on the plate (Bambu Studio 2.7.1.62, September 2026). An embossed logo in this layout would land on the bed beside its tile. **Unconfirmed:** whether Bambu Studio asks a multi-part question for one 3MF with several objects, as PrusaSlicer does. Forum posts describe the question for several STL files dropped at once (2023).

**(c) Colour groups are read and turned into painting (source and reported).** The importer reads `m:colorgroup` and `m:color`.
- For an object without Bambu metadata whose own `pid` names a colour group, each distinct colour is mapped to an extruder number starting at 1, and set as the object's `extruder`.
- For non-Bambu files, per-triangle colours (`pid`, `p1`…`p3`, or the object's `pid` when a triangle has none) are collected per volume.

Bambu Studio 2.5.0 added "importing standard 3MF files with color information. After import, the workflow is similar to multi-color OBJ: select the color data to map and apply colors" (release notes, reported). Since then, users report that the dialog turns object and body colours into surface painting, not per-part filament assignment:
- on a Fusion 360 export with one colour per body, the colours are painted onto the whole model, and per-part filament choices are ignored until the paint is erased;
- colour bleeds into the model's interior layers;
- users cancel the dialog, or undo right after import, to avoid this.

Bambu Lab acknowledged the bug in [issue #9666](https://github.com/bambulab/BambuStudio/issues/9666). It is still open: a collaborator wrote on 19 October 2026 that a release at the beginning of November "will include the handling for this issue". 2.8.2 also fixed "incorrect slicing colors bleeding into adjacent objects when importing standard multi-color 3MF files". **Unconfirmed:** which of these paths a 3MF in layout (b) takes. Its colours sit on the part meshes, not on the components object, which the specification forbids from carrying `pid`. So it most likely takes the per-triangle path, and with it the painting dialog.

**OrcaSlicer**, a fork of Bambu Studio, was not researched.

## 3MF Core 1.4.0

[3MF Core specification 1.4.0](https://github.com/3MFConsortium/spec_core/blob/master/3MF%20Core%20Specification.md), published 6 February 2025:

- Build items: "consumers MAY rearrange the items" (section 3.4.3.1).
- Components: "A 3D manufacturing device MUST respect the relative positions of the component objects; it MUST NOT transform them relative to each other except as specified in the document" (section 4.2).
- "Producers MUST NOT assign pid or pindex attributes to objects that contain components" (chapter 4), so colours belong on the part meshes.
- "Producers MUST define objects prior to referencing them as components" (section 4.2.1).
- Items and components "SHOULD NOT overlap"; where they do, consumers unite them and the last one's properties win.

## What OpenBrick3D does now

- **Writer.** `openbrick_io::write_3mf_with_layout(objects, meta, layout)` and `threemf_model_xml_with_layout` take an `ExportLayout`.
  - `SeparateObjects` is the default and what `write_3mf` always wrote: one mesh object and one build item per `ExportObject`.
  - `OneObjectWithParts` writes the same mesh objects. After them comes one components object, named after the title (or the part id), with one `<component>` per mesh in input order, no transform and no `pid`. One build item places it. With a single object it writes exactly the `SeparateObjects` package.
  - `ExportLayout::PART_DESIGNER` is `OneObjectWithParts`.
  - Production UUIDs are on the build, every item, object and component. They are derived as before. A grouped package adds `layout=oneObjectWithParts` and `assembly=<len>:<name>` to the seed and derives `assembly`, `component/i` and `item/0`, so the two layouts never share UUIDs. `scripts/validate-3mf.py` re-derives them from what lib3mf reads.
- **Validation.** `scripts/validate-3mf.py` accepts the components layout, which lib3mf 2.5.0 reads in strict mode without warnings. It requires:
  - every mesh to be placed exactly once, directly or as a part;
  - identity transforms on components and items;
  - a UUID on every component;
  - no two mesh objects to overlap, sampled on 64 × 64 vertical lines over the region where their boxes overlap.

  `--self-test` adds broken component copies: overlapping parts, a component with a transform, a component without a UUID, a part placed twice, a part left out. The fixture `stacked-parts` (`write_fixtures`) uses the new layout. The part crate writes `logo-tile-emboss-parts.3mf`, `logo-tile-inlay-parts.3mf` and `logo-tile-mark-inlay-parts.3mf` for the same checks.
- **Evaluator.** `relief.svg` gains `inlayStyle`: `swap` (the default, unchanged) or `parts`.
  - `parts` builds the multi-material inlay: the logo fill is its own `accent` object, `height` deep in the face, and the body is recessed by exactly that region. The two add up to the plain tile, and the kernel finds no shared volume.
  - Every multi-colour part gets a warning naming its colour roles, what each holds, and which face goes on the bed.

## Recommendations and open decisions

1. **Part-designer exports with more than one colour role:** use `ExportLayout::PART_DESIGNER`. It is the only layout that keeps the parts in place in Bambu Studio, and PrusaSlicer loads it no worse than separate items. Keep `SeparateObjects` for single-object exports and for callers that place objects side by side (the assembler's own plates).
2. **Colours (a decision for a human).** `docs/plan/05` section 7 writes one colour group entry per role. PrusaSlicer ignores it. Bambu Studio 2.5 to 2.8.2 turns it into painting, which is reported to override per-part filaments. Until a print test or Bambu's announced fix shows otherwise:
   - tell Bambu users to choose Cancel in the "Standard 3MF Import color" dialog and then pick a filament for each part;
   - consider letting the CLI omit colours for Bambu Studio (`ExportObject::color_srgb = None` already writes no colour group).
3. **Face down.** An inlay prints with the top face on the bed. Exports keep the design orientation, and the evaluator's warning tells the user to turn the part over. One components object makes that a single rotation in either slicer, where several build items would have to be turned one by one. A follow-up could write the flip (180 degrees about x, then lifted onto the bed) into the build item's transform. Both slicers read item transforms (source). That needs a rule change, because the validator and `docs/plan/05` section 7 currently require identity transforms.
4. **Names.** PrusaSlicer shows the source objects' names on the merged parts, so exporters should pass meaningful names such as `ndn.logo-tile body` and `ndn.logo-tile accent`.

## Print test to close this question

Open the CI artifact `part-exports` (`logo-tile-emboss-parts.3mf`, `logo-tile-mark-inlay-parts.3mf`) in:
- Bambu Studio 2.8.2 with the A1 and AMS lite;
- PrusaSlicer 2.9.6 with the XL.

Record, with screenshots, under this question and Q06:
- whether the file loads as one object with two parts;
- whether the logo stays on (or in) the face;
- which dialogs appear (Bambu's colour dialog; PrusaSlicer's multi-part question);
- which filaments or extruders the parts get;
- whether Cancel in Bambu's dialog leaves per-part filaments working;
- whether the sliced preview shows the logo colour only in the logo.

## Sources

All read on 2026-10-28.

- PrusaSlicer source at tag `version_2.9.6`:
  - [`src/libslic3r/Format/3mf.cpp`](https://github.com/prusa3d/PrusaSlicer/blob/version_2.9.6/src/libslic3r/Format/3mf.cpp)
  - [`src/libslic3r/FileReader.cpp`](https://github.com/prusa3d/PrusaSlicer/blob/version_2.9.6/src/libslic3r/FileReader.cpp)
  - [`src/libslic3r/ModelProcessing.cpp`](https://github.com/prusa3d/PrusaSlicer/blob/version_2.9.6/src/libslic3r/ModelProcessing.cpp)
- PrusaSlicer source at tag `version_2.9.2`:
  - [`src/slic3r/GUI/Plater.cpp`](https://github.com/prusa3d/PrusaSlicer/blob/version_2.9.2/src/slic3r/GUI/Plater.cpp)
  - [`3mf.cpp`](https://github.com/prusa3d/PrusaSlicer/blob/version_2.9.2/src/libslic3r/Format/3mf.cpp)
- [PrusaSlicer releases](https://github.com/prusa3d/PrusaSlicer/releases): 2.9.6 of 25 June 2026; 3.0.0-alpha12 of 21 September 2026.
- [Prusa help: importing a multi-material model](https://help.prusa3d.com/article/importing-multi-material-model_121191) (search-result excerpt only).
- Bambu Studio source on `master`: [`src/libslic3r/Format/bbs_3mf.cpp`](https://github.com/bambulab/BambuStudio/blob/master/src/libslic3r/Format/bbs_3mf.cpp).
- [Bambu Studio releases](https://github.com/bambulab/BambuStudio/releases): 2.8.2.61 is the latest public release; 2.8.3 and 2.8.4 are betas.
- [Bambu Studio v02.05.00.64 release notes](https://newreleases.io/project/github/bambulab/BambuStudio/release/v02.05.00.64): standard 3MF colour import.
- [bambulab/BambuStudio#9666](https://github.com/bambulab/BambuStudio/issues/9666): 3MF colour import paints instead of assigning filaments; opened 4 February 2026, open on 27 October 2026.
- Bambu Lab forum threads:
  - [Solid body colors replaced by surface painting](https://forum.bambulab.com/t/solid-body-colors-replaced-by-surface-painting-please-let-us-turn-this-off/229799) (30 January 2026, Bambu Studio 2.5.0.66);
  - [Import .3mf as part not object](https://forum.bambulab.com/t/import-3mf-as-part-not-object/260189) (24 to 26 September 2026, Bambu Studio 2.7.1.62);
  - [Importing Blender multi color 3mf file into Bambu Studio is problematic](https://forum.bambulab.com/t/importing-blender-multi-color-3mf-file-into-bambu-studio-is-problematic/242163) (March to April 2026);
  - [Multipart models](https://forum.bambulab.com/t/multipart-models/5712) (February 2023).
- [3MF Core specification 1.4.0](https://github.com/3MFConsortium/spec_core/blob/master/3MF%20Core%20Specification.md) (6 February 2025).
- Not opened: Bambu Lab's wiki pages "Standard 3MF File Color Parsing" and "3mf compatibility" answered HTTP 402.
- Not used: [a blog post on multi-colour 3MF export](https://modelrift.com/blog/multicolor-3mf-export/) (18 May 2026) says Bambu Studio matches `basematerials` display colours to loaded filaments by RGB distance. That disagrees with the painting reports above and is not confirmed.
