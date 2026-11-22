"""Every rule of check_structure.py, and every pattern and name it lists, fails on a small fixture tree
built in a temporary folder with one deliberate violation. No real file is edited. (`./gradlew check`
runs the script on the real tree.)"""

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_structure  # noqa: E402

MOBILE = Path(__file__).resolve().parent.parent
SCRIPT = MOBILE / "tools" / "check_structure.py"

BUILD = 'dependencies {\n    api(project(":core:construct"))\n%s}\n'
KOTLIN_SCREEN_PATH = "feature/notes/src/androidMain/kotlin/cf/construct/mobile/feature/notes/NotesScreen.kt"
SWIFT_SCREEN_PATH = "iosApp/Features/Notes/NotesScreen.swift"
KOTLIN_SCREEN = "package cf.construct.mobile.feature.notes\n\nfun NotesScreen() {\n%s\n}\n"
SWIFT_SCREEN = "import SwiftUI\n\nstruct NotesScreen: View {\n    var body: some View {\n%s\n    }\n}\n"


def tree(test: unittest.TestCase, files: dict[str, str]) -> Path:
    """A clean minimal tree (one Kotlin feature `notes` and the same on the iPhone) plus `files`."""
    root = Path(tempfile.mkdtemp())
    test.addCleanup(shutil.rmtree, root, ignore_errors=True)
    base = {
        "feature/notes/build.gradle.kts": BUILD % "",
        KOTLIN_SCREEN_PATH: KOTLIN_SCREEN % '    Text("ok")',
        "feature/notes/src/commonMain/kotlin/cf/construct/mobile/feature/notes/NotesState.kt": (
            "package cf.construct.mobile.feature.notes\n\ndata class NotesState(val items: List<String>)\n"
        ),
        SWIFT_SCREEN_PATH: SWIFT_SCREEN % '        Text("ok")',
        "iosApp/Features/Notes/NotesPage.swift": "import ConstructShared\n\nstruct NotesPage { let c: Construct }\n",
    }
    for relative, text in {**base, **files}.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    # A feature of a fixture that is not about screens gets a clean screen of its own.
    for feature in (root / "feature").iterdir():
        if not any(feature.glob("src/*Main/**/*Screen*.kt")) and (feature / "build.gradle.kts").is_file():
            stub = feature / f"src/androidMain/kotlin/{feature.name}Screen.kt"
            stub.parent.mkdir(parents=True, exist_ok=True)
            stub.write_text(f"fun {feature.name}Screen() {{}}\n", encoding="utf-8")
    for feature in (root / "iosApp/Features").iterdir():
        if not any("Screen" in p.name for p in feature.rglob("*.swift")):
            (feature / f"{feature.name}Screen.swift").write_text(
                f"struct {feature.name}Screen {{}}\n", encoding="utf-8"
            )
    return root


class Base(unittest.TestCase):
    def found(self, files: dict[str, str]) -> list[str]:
        return check_structure.run(tree(self, files))

    def assertFails(self, files: dict[str, str], path: str, text: str) -> None:
        found = self.found(files)
        self.assertTrue(any(line.startswith(path + ":") and text in line for line in found), found)

    def assertPasses(self, files: dict[str, str]) -> None:
        self.assertEqual(self.found(files), [])


class CleanTrees(Base):
    def test_the_minimal_tree_passes_before_it_is_broken(self):
        self.assertPasses({})

    def test_a_copy_of_the_template_passes_with_no_other_file_changed(self):
        root = tree(self, {})
        copy = root / "feature" / "notes"
        shutil.rmtree(copy)
        shutil.copytree(MOBILE / "feature" / "template", copy, ignore=shutil.ignore_patterns("build"))
        for path in sorted(copy.rglob("*"), key=lambda p: -len(p.parts)):
            if path.is_file():
                text = path.read_text(encoding="utf-8").replace("template", "notes").replace("Template", "Notes")
                path.write_text(text, encoding="utf-8")
            if "emplate" in path.name:
                path.rename(path.with_name(path.name.replace("template", "notes").replace("Template", "Notes")))

        def others() -> dict[Path, str]:
            return {p: p.read_text(encoding="utf-8") for p in root.rglob("*") if p.is_file() and copy not in p.parents}

        before = others()
        self.assertEqual(check_structure.run(root), [])
        self.assertEqual(before, others())
        self.assertTrue((copy / "src/androidMain/kotlin/cf/construct/mobile/feature/notes/NotesScreen.kt").is_file())


class CommandLine(Base):
    def run_script(self, root: Path, *extra: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, "-I", str(SCRIPT), "--root", str(root), *extra],
            capture_output=True,
            text=True,
            check=False,
        )

    def test_a_clean_tree_exits_0_and_prints_nothing(self):
        result = self.run_script(tree(self, {}))
        self.assertEqual((result.returncode, result.stdout), (0, ""))

    def test_a_violation_exits_1_and_prints_file_and_line(self):
        root = tree(self, {KOTLIN_SCREEN_PATH: KOTLIN_SCREEN % "    padding(16.dp)"})
        result = self.run_script(root)
        self.assertEqual(result.returncode, 1)
        self.assertRegex(result.stdout, r"NotesScreen\.kt:4: a screen has a size literal: '16\.dp'")

    def test_a_folder_that_is_not_there_exits_1_instead_of_passing(self):
        result = self.run_script(Path("/nonexistent-mobile-root"))
        self.assertEqual(result.returncode, 1)
        self.assertIn("nothing to scan", result.stdout)

    def test_a_tree_without_kotlin_features_or_without_swift_features_exits_1(self):
        for folder in ("feature", "iosApp"):
            with self.subTest(missing=folder):
                root = tree(self, {})
                shutil.rmtree(root / folder)
                result = self.run_script(root)
                self.assertEqual(result.returncode, 1)
                self.assertIn("nothing to scan", result.stdout)

    def test_a_feature_without_a_screen_exits_1_so_a_rename_cannot_hide_a_screen(self):
        for old, new in ((KOTLIN_SCREEN_PATH, "ListView.kt"), (SWIFT_SCREEN_PATH, "NotesView.swift")):
            with self.subTest(old=old):
                root = tree(self, {})
                (root / old).rename((root / old).with_name(new))
                result = self.run_script(root)
                self.assertEqual(result.returncode, 1)
                self.assertIn("needs a *Screen*", result.stdout)


class FeatureUsesNoFeature(Base):
    def test_gradle_dependencies_on_another_feature_fail_with_the_file(self):
        for dep in (
            'api(project(":feature:other"))',
            'implementation(project(path = ":feature:other"))',
            "api(projects.feature.other)",
        ):
            with self.subTest(dep=dep):
                self.assertFails(
                    {"feature/notes/build.gradle.kts": BUILD % f"    {dep}\n"},
                    "feature/notes/build.gradle.kts",
                    "depends on feature 'other'",
                )

    def test_a_dependency_on_a_core_module_and_on_itself_passes(self):
        self.assertPasses({"feature/notes/build.gradle.kts": BUILD % '    api(project(":feature:notes"))\n'})

    def test_a_glob_inside_a_string_does_not_hide_a_later_dependency(self):
        build = BUILD % '    exclude("**/generated/**")\n    api(project(":feature:other"))\n'
        self.assertFails(
            {"feature/notes/build.gradle.kts": build}, "feature/notes/build.gradle.kts", "depends on feature 'other'"
        )

    def test_a_commented_dependency_passes(self):
        self.assertPasses({"feature/notes/build.gradle.kts": BUILD % '    // api(project(":feature:other"))\n'})

    def test_kotlin_use_of_another_feature_fails_as_an_import_and_written_in_full(self):
        for use in (
            "import cf.construct.mobile.feature.other.OtherState\n",
            "val s: cf.construct.mobile.feature.other.OtherState? = null\n",
        ):
            with self.subTest(use=use):
                path = "feature/notes/src/commonMain/kotlin/cf/construct/mobile/feature/notes/Uses.kt"
                self.assertFails(
                    {path: "package cf.construct.mobile.feature.notes\n\n" + use}, path, "uses feature 'other'"
                )

    def test_a_module_without_a_build_file_is_not_a_feature(self):
        self.assertPasses({"feature/draft/src/Stray.kt": "import cf.construct.mobile.feature.other.X\n"})

    def test_a_swift_feature_naming_a_swift_type_of_another_feature_fails(self):
        files = {
            "iosApp/Features/Other/OtherView.swift": "struct OtherView {}\n",
            "iosApp/Features/Notes/Uses.swift": "struct Uses {\n    let v = OtherView()\n}\n",
        }
        self.assertFails(files, "iosApp/Features/Notes/Uses.swift", "uses 'OtherView' of feature 'other'")

    def test_a_swift_feature_naming_a_kotlin_type_of_another_feature_fails(self):
        files = {
            "feature/other/build.gradle.kts": BUILD % "",
            "feature/other/src/commonMain/kotlin/cf/construct/mobile/feature/other/OtherHolder.kt": (
                "package cf.construct.mobile.feature.other\n\nclass OtherHolder\n"
            ),
            "iosApp/Features/Notes/Uses.swift": "struct Uses {\n    let h: OtherHolder? = nil\n}\n",
        }
        self.assertFails(files, "iosApp/Features/Notes/Uses.swift", "uses 'OtherHolder' of feature 'other'")

    def test_a_private_or_nested_type_of_another_feature_is_not_counted(self):
        files = {
            "iosApp/Features/Other/OtherView.swift": (
                "private struct Row {}\nstruct OtherView {\n    struct Cell {}\n    class func make() {}\n}\n"
            ),
            "iosApp/Features/Notes/Uses.swift": "struct Uses {\n    let r = Row(), c = Cell(), f = make\n}\n",
        }
        self.assertPasses(files)

    def test_a_feature_names_its_own_private_or_nested_type_even_when_another_feature_declares_it(self):
        files = {
            "iosApp/Features/Other/Row.swift": "struct Row {}\n",
            "iosApp/Features/Notes/Own.swift": "struct Own {\n    private struct Row {}\n    let r = Row()\n}\n",
        }
        self.assertPasses(files)

    def test_attributes_on_the_declaration_line_do_not_hide_a_type(self):
        for declaration in (
            "@MainActor final class OtherModel {}",
            "@MainActor\nfinal class OtherModel {}",
            "typealias OtherModel = Int",
        ):
            with self.subTest(declaration=declaration):
                files = {
                    "iosApp/Features/Other/OtherModel.swift": declaration + "\n",
                    "iosApp/Features/Notes/Uses.swift": "struct Uses {\n    let m: OtherModel? = nil\n}\n",
                }
                self.assertFails(files, "iosApp/Features/Notes/Uses.swift", "uses 'OtherModel'")

    def test_kotlin_types_of_every_declaration_kind_count_and_private_or_nested_ones_do_not(self):
        other = "feature/other/src/commonMain/kotlin/Other.kt"
        for kind, name in (
            ("class", "OtherClass"),
            ("interface", "OtherInterface"),
            ("object", "OtherObject"),
            ("data class", "OtherData"),
            ("@Immutable data class", "OtherAnnotated"),
            ("annotation class", "OtherAnnotation"),
            ("typealias", "OtherAlias"),
        ):
            with self.subTest(kind=kind):
                text = f"{kind} {name}" + (" = Int" if kind == "typealias" else "") + "\n"
                files = {
                    "feature/other/build.gradle.kts": BUILD % "",
                    other: text,
                    "iosApp/Features/Notes/Uses.swift": f"struct Uses {{\n    let v: {name}? = nil\n}}\n",
                }
                self.assertFails(files, "iosApp/Features/Notes/Uses.swift", f"uses '{name}'")
        files = {
            "feature/other/build.gradle.kts": BUILD % "",
            other: "private class Hidden\nclass Outer {\n    class Inner\n}\n",
            "iosApp/Features/Notes/Uses.swift": "struct Uses {\n    let a = Hidden(), b = Inner()\n}\n",
        }
        self.assertPasses(files)

    def test_multi_word_feature_names_are_one_feature_in_kotlin_swift_and_gradle(self):
        files = {
            "feature/shopping_cart/build.gradle.kts": BUILD % '    api(project(":feature:shopping-cart"))\n',
            "feature/shopping_cart/src/commonMain/kotlin/Cart.kt": "class ShoppingCartHolder\n",
            "iosApp/Features/ShoppingCart/Page.swift": "struct Page {\n    let h: ShoppingCartHolder? = nil\n}\n",
        }
        self.assertPasses(files)

    def test_a_type_both_features_declare_is_not_cross_use(self):
        files = {
            "iosApp/Features/Other/Row.swift": "struct Row {}\n",
            "iosApp/Features/Notes/Row.swift": "struct Row {}\nlet r = Row()\n",
        }
        self.assertPasses(files)


class ScreenNamesNoStore(Base):
    def test_kotlin_imports_of_the_interface_and_the_database_fail_with_the_file(self):
        for package in ("construct.Construct", "database.CopyStore"):
            with self.subTest(package=package):
                text = (
                    "package cf.construct.mobile.feature.notes\n\n"
                    f"import cf.construct.mobile.core.{package}\n\nfun NotesScreen() {{}}\n"
                )
                self.assertFails({KOTLIN_SCREEN_PATH: text}, KOTLIN_SCREEN_PATH, "imports a store")

    def test_a_kotlin_screen_in_any_main_source_set_is_checked(self):
        path = "feature/notes/src/commonMain/kotlin/cf/construct/mobile/feature/notes/ListScreen.kt"
        self.assertFails({path: KOTLIN_SCREEN % "    val h: NotesHolder"}, path, "'NotesHolder'")

    def test_names_of_stores_in_a_screen_fail(self):
        for name in (
            "Construct",
            "FakeConstruct",
            "NotesHolder",
            "NotesApi",
            "NotesRepository",
            "NotesStore",
            "NotesDao",
            "NotesDatabase",
            "NotesModel",
            "UiScope",
        ):
            with self.subTest(name=name):
                self.assertFails(
                    {SWIFT_SCREEN_PATH: SWIFT_SCREEN % f"        let x: {name}"}, SWIFT_SCREEN_PATH, f"'{name}'"
                )
                self.assertFails(
                    {KOTLIN_SCREEN_PATH: KOTLIN_SCREEN % f"    val x: {name}"}, KOTLIN_SCREEN_PATH, f"'{name}'"
                )

    def test_a_material_opt_in_is_not_a_store(self):
        text = (
            "import androidx.compose.material3.ExperimentalMaterial3Api\n\n"
            "@OptIn(ExperimentalMaterial3Api::class)\nfun NotesScreen() {}\n"
        )
        self.assertPasses({KOTLIN_SCREEN_PATH: text})

    def test_the_experimental_exemption_is_only_for_experimental_apis(self):
        for name in ("ExperimentalHolder", "ExperimentalConstruct"):
            with self.subTest(name=name):
                self.assertFails(
                    {KOTLIN_SCREEN_PATH: KOTLIN_SCREEN % f"    val x: {name}"}, KOTLIN_SCREEN_PATH, f"'{name}'"
                )

    def test_the_shared_framework_the_theme_and_a_state_pass(self):
        screen = (
            "import ConstructShared\nimport SwiftUI\n\nstruct NotesScreen: View {\n    let state: NotesState\n"
            "    var body: some View { Text(ConstructTheme.title) }\n}\n"
        )
        self.assertPasses({SWIFT_SCREEN_PATH: screen})

    def test_a_comment_or_a_string_that_names_a_store_passes(self):
        text = SWIFT_SCREEN % '        // Talks to Construct.\n        Text("ask the NotesHolder")'
        self.assertPasses({SWIFT_SCREEN_PATH: text})

    def test_only_files_named_screen_are_held_to_the_rule(self):
        # NotesPage.swift of the minimal tree takes a `Construct` and passes.
        self.assertPasses({})
        self.assertFails(
            {"iosApp/Features/Notes/NotesScreen.swift": SWIFT_SCREEN % "        let c: Construct"},
            SWIFT_SCREEN_PATH,
            "'Construct'",
        )


KOTLIN_LITERAL_CASES = [
    ("    padding(16.dp)", "size literal"),
    ("    padding(16f.dp)", "size literal"),
    ("    Text(fontSize = 14.sp)", "size literal"),
    ("    Text(fontSize = 1.5.sp)", "size literal"),
    ("    Box(Modifier.size(Dp(8f)))", "built in the screen"),
    ("    Text(color = Color(0xFFB00020))", "built in the screen"),
    ("    val c = 0xFFB00020", "colour literal"),
    ("    Text(color = Color.Red)", "colour literal"),
]
SWIFT_LITERAL_CASES = [
    ("        Text().foregroundColor(Color(red: 1, green: 0, blue: 0))", "built in the screen"),
    ("        Text().foregroundColor(UIColor(white: 1, alpha: 1))", "built in the screen"),
    ("        Text().foregroundColor(Color.red)", "colour literal"),
    ("        Text().foregroundColor(.red)", "colour literal"),
    ("        Text().background(.white)", "colour literal"),
    ("        Circle().stroke(.black)", "colour literal"),
    ("        let c = #colorLiteral(red: 1, green: 0, blue: 0, alpha: 1)", "colour literal"),
    ("        Text().frame(width: 320)", "size literal"),
    ("        Text().font(.system(size: 14))", "size literal"),
    ("        VStack(spacing: 8) {}", "size literal"),
    ("        Text().frame(width: CGFloat(24))", "size literal"),
    ("        Text().padding(EdgeInsets(top: 1, leading: 1, bottom: 1, trailing: 1))", "size literal"),
    ("        Text().padding(8)", "size literal"),
    ("        Text().padding(.horizontal, 8)", "size literal"),
    ("        Text().cornerRadius(4)", "size literal"),
]


class ScreenHasNoLiteral(Base):
    def test_every_kotlin_literal_pattern_fails_with_the_file_and_the_whole_literal(self):
        for body, label in KOTLIN_LITERAL_CASES:
            with self.subTest(body=body):
                self.assertFails({KOTLIN_SCREEN_PATH: KOTLIN_SCREEN % body}, KOTLIN_SCREEN_PATH, label)

    def test_every_swift_literal_pattern_fails_with_the_file(self):
        for body, label in SWIFT_LITERAL_CASES:
            with self.subTest(body=body):
                self.assertFails({SWIFT_SCREEN_PATH: SWIFT_SCREEN % body}, SWIFT_SCREEN_PATH, label)

    def test_the_message_quotes_the_whole_number(self):
        found = self.found({SWIFT_SCREEN_PATH: SWIFT_SCREEN % "        Text().frame(width: 320)"})
        self.assertIn("'width: 320'", found[0])

    def test_theme_values_ratios_and_infinity_pass(self):
        kotlin = "    padding(Spacing.L.dp)\n    Modifier.weight(1f)\n    Text(color = MaterialTheme.colorScheme.error)"
        swift = (
            "        Text().frame(maxWidth: .infinity)\n"
            "        Text().padding(.vertical, ConstructTheme.Space.small)\n"
            "        Text().foregroundColor(ConstructTheme.color(.primary))\n"
            "        Text().background(.regularMaterial)"
        )
        self.assertPasses({KOTLIN_SCREEN_PATH: KOTLIN_SCREEN % kotlin, SWIFT_SCREEN_PATH: SWIFT_SCREEN % swift})

    def test_a_literal_outside_a_screen_file_passes(self):
        self.assertPasses({"iosApp/Features/Notes/NotesRow.swift": SWIFT_SCREEN % "        Text().padding(8)"})


COLOURS = "red orange yellow green mint teal cyan blue indigo purple pink brown white gray black".split()
COLOUR_MODIFIERS = "foregroundColor foregroundStyle background tint fill stroke".split()
SIZE_NAMES = "size width height minWidth maxWidth minHeight maxHeight spacing radius cornerRadius lineWidth".split()


class EveryAlternative(Base):
    """Each name a pattern lists is guarded by a case of its own."""

    def swift(self, body: str, text: str) -> None:
        self.assertFails({SWIFT_SCREEN_PATH: SWIFT_SCREEN % f"        {body}"}, SWIFT_SCREEN_PATH, text)

    def test_every_swift_colour_name_is_refused(self):
        for colour in COLOURS:
            with self.subTest(colour=colour):
                self.swift(f"Text().foregroundColor(.{colour})", "colour literal")

    def test_every_swift_colour_modifier_is_checked(self):
        for modifier in COLOUR_MODIFIERS:
            with self.subTest(modifier=modifier):
                self.swift(f"Text().{modifier}(.red)", "colour literal")

    def test_every_swift_size_argument_name_is_checked(self):
        for name in SIZE_NAMES:
            with self.subTest(name=name):
                self.swift(f"Text().frame({name}: 3)", "size literal")

    def test_every_swift_constructor_and_modifier_of_a_size_is_checked(self):
        cases = [
            "CGSize(width: 1, height: 2)",
            "CGFloat(3)",
            "EdgeInsets(top: 1)",
            "Text().offset(x: 3)",
            "Text().offset(y: 3)",
        ]
        for body in cases:
            with self.subTest(body=body):
                self.swift(body, "size literal")

    def test_every_swift_colour_constructor_is_refused_and_clear_is_not(self):
        for body in (
            "Color(red: 1)",
            "UIColor(white: 1, alpha: 1)",
            "NSColor(white: 1, alpha: 1)",
            "Color.green",
            "UIColor.blue",
        ):
            with self.subTest(body=body):
                self.swift(f"let c = {body}", "colour")
        self.assertPasses({SWIFT_SCREEN_PATH: SWIFT_SCREEN % "        Text().background(Color.clear)"})

    def test_every_kotlin_builder_is_refused(self):
        for body in ("Color(0xFF000000)", "Dp(8f)", "TextUnit(8f, TextUnitType.Sp)"):
            with self.subTest(body=body):
                self.assertFails(
                    {KOTLIN_SCREEN_PATH: KOTLIN_SCREEN % f"    val x = {body}"},
                    KOTLIN_SCREEN_PATH,
                    "built in the screen",
                )

    def test_a_kotlin_colour_in_each_literal_length_is_refused(self):
        for literal in ("0xB00020", "0xFFB00020"):
            with self.subTest(literal=literal):
                self.assertFails(
                    {KOTLIN_SCREEN_PATH: KOTLIN_SCREEN % f"    val x = {literal}"}, KOTLIN_SCREEN_PATH, "colour literal"
                )


class WhereTheScreensAre(Base):
    def test_a_screen_with_a_longer_name_is_still_checked(self):
        for path, body in (
            ("feature/notes/src/commonMain/kotlin/NotesScreenContent.kt", "fun C() { padding(16.dp) }\n"),
            ("iosApp/Features/Notes/NotesScreenContent.swift", "struct C {\n    let x = Text().padding(8)\n}\n"),
        ):
            with self.subTest(path=path):
                self.assertFails({path: body}, path, "size literal")


KOTLIN_MODIFIER_LINES = [
    "public class OtherType",
    "sealed class OtherType",
    "abstract class OtherType",
    "open class OtherType",
    "enum class OtherType",
    "value class OtherType",
    "fun interface OtherType",
    "expect class OtherType",
    "actual class OtherType",
    "final class OtherType",
    "external interface OtherType",
    "data object OtherType",
]
SWIFT_MODIFIER_LINES = [
    "public struct OtherType {}",
    "open class OtherType {}",
    "final class OtherType {}",
    "enum OtherType {}",
    "protocol OtherType {}",
    "actor OtherType {}",
    "indirect enum OtherType {}",
    "nonisolated struct OtherType {}",
    "package struct OtherType {}",
]


class EveryDeclaration(Base):
    """Each kind of declaration and each modifier is seen as a public top-level type."""

    def use(self, declaration_files: dict[str, str]) -> None:
        files = {
            **declaration_files,
            "iosApp/Features/Notes/Uses.swift": "struct Uses {\n    let v: OtherType? = nil\n}\n",
        }
        self.assertFails(files, "iosApp/Features/Notes/Uses.swift", "uses 'OtherType'")

    def test_every_kotlin_modifier_and_kind(self):
        for line in KOTLIN_MODIFIER_LINES:
            with self.subTest(line=line):
                self.use({"feature/other/build.gradle.kts": BUILD % "", "feature/other/src/Other.kt": line + "\n"})

    def test_every_swift_modifier_and_kind(self):
        for line in SWIFT_MODIFIER_LINES:
            with self.subTest(line=line):
                self.use({"iosApp/Features/Other/Other.swift": line + "\n"})

    def test_a_hidden_kotlin_type_is_not_seen(self):
        for line in ("private class OtherType", "internal class OtherType", "protected class OtherType"):
            with self.subTest(line=line):
                files = {"feature/other/build.gradle.kts": BUILD % "", "feature/other/src/Other.kt": line + "\n"}
                files["iosApp/Features/Notes/Uses.swift"] = "struct Uses {\n    let v: OtherType? = nil\n}\n"
                self.assertPasses(files)

    def test_a_hidden_swift_type_of_another_feature_is_not_seen(self):
        for line in (
            "private struct OtherType {}",
            "fileprivate struct OtherType {}",
            "final private class OtherType {}",
            "final fileprivate class OtherType {}",
            "private final class OtherType {}",
        ):
            with self.subTest(line=line):
                files = {"iosApp/Features/Other/Other.swift": line + "\n"}
                files["iosApp/Features/Notes/Uses.swift"] = "struct Uses {\n    let v: OtherType? = nil\n}\n"
                self.assertPasses(files)

    def test_an_explicit_internal_swift_type_is_seen_because_it_is_the_default(self):
        self.use({"iosApp/Features/Other/Other.swift": "internal struct OtherType {}\n"})

    def test_a_nested_type_on_a_later_line_is_not_a_top_level_type_of_the_line_above(self):
        files = {
            "iosApp/Features/Other/Other.swift": "extension\n    struct OtherType {}\n",
            "iosApp/Features/Notes/Uses.swift": "struct Uses {\n    let v: OtherType? = nil\n}\n",
        }
        self.assertPasses(files)

    def test_a_private_type_of_one_file_does_not_cover_another_file_of_the_feature(self):
        for access in ("private", "fileprivate"):
            with self.subTest(access=access):
                files = {
                    "iosApp/Features/Other/State.swift": "struct MemoryState {}\n",
                    "iosApp/Features/Notes/A.swift": f"{access} struct MemoryState {{}}\n",
                    "iosApp/Features/Notes/B.swift": "struct B {\n    let s: MemoryState? = nil\n}\n",
                }
                self.assertFails(files, "iosApp/Features/Notes/B.swift", "uses 'MemoryState'")

    def test_a_nested_type_covers_its_own_file_but_not_the_next_one(self):
        other = {"iosApp/Features/Other/State.swift": "struct MemoryState {}\n"}
        own = {
            "iosApp/Features/Notes/A.swift": "struct A {\n    struct MemoryState {}\n    let s: MemoryState? = nil\n}\n"
        }
        self.assertPasses({**other, **own})
        next_file = {"iosApp/Features/Notes/B.swift": "struct B {\n    let s: MemoryState? = nil\n}\n"}
        self.assertFails({**other, **own, **next_file}, "iosApp/Features/Notes/B.swift", "uses 'MemoryState'")

    def test_a_hidden_type_covers_its_own_file(self):
        for access in ("private", "fileprivate"):
            with self.subTest(access=access):
                files = {
                    "iosApp/Features/Other/State.swift": "struct MemoryState {}\n",
                    "iosApp/Features/Notes/A.swift": f"{access} struct MemoryState {{}}\nlet s: MemoryState? = nil\n",
                }
                self.assertPasses(files)

    def test_a_kotlin_nested_type_of_the_feature_is_not_another_features_bare_name(self):
        files = {
            "iosApp/Features/Other/State.swift": "struct MemoryState {}\n",
            "feature/notes/src/commonMain/Notes.kt": "class Outer {\n    class MemoryState\n}\n",
            "iosApp/Features/Notes/Uses.swift": "struct Uses {\n    let s: MemoryState? = nil\n}\n",
        }
        self.assertPasses(files)

    def test_a_hidden_kotlin_type_of_the_using_feature_does_not_cover_a_name_swift_cannot_see(self):
        for access in ("private", "internal", "protected"):
            with self.subTest(access=access):
                files = {
                    "iosApp/Features/Other/State.swift": "struct MemoryState {}\n",
                    "feature/notes/src/commonMain/Notes.kt": f"{access} class MemoryState\n",
                    "iosApp/Features/Notes/Uses.swift": "struct Uses {\n    let s: MemoryState? = nil\n}\n",
                }
                self.assertFails(files, "iosApp/Features/Notes/Uses.swift", "uses 'MemoryState'")

    def test_a_kotlin_modifier_line_does_not_join_the_indented_type_below_it(self):
        files = {
            "feature/other/build.gradle.kts": BUILD % "",
            "feature/other/src/Other.kt": "expect\n    class OtherType\n",
            "iosApp/Features/Notes/Uses.swift": "struct Uses {\n    let v: OtherType? = nil\n}\n",
        }
        self.assertPasses(files)

    def test_an_attribute_between_the_modifiers_is_read(self):
        for line in ("public @MainActor final class OtherType {}", "final @MainActor class OtherType {}"):
            with self.subTest(line=line):
                self.use({"iosApp/Features/Other/Other.swift": line + "\n"})

    def test_a_private_type_with_an_attribute_between_the_modifiers_is_the_files_own(self):
        # Seen as hidden, the type covers its own file; unread, it would leave the use below to fail.
        files = {
            "iosApp/Features/Other/Other.swift": "struct OtherType {}\n",
            "iosApp/Features/Notes/A.swift": "private @MainActor struct OtherType {}\nlet v: OtherType? = nil\n",
        }
        self.assertPasses(files)

    def test_a_swift_file_below_the_feature_folder_is_scanned(self):
        path = "iosApp/Features/Notes/Sub/Deep.swift"
        files = {
            "iosApp/Features/Other/Other.swift": "struct OtherType {}\n",
            path: "struct Deep {\n    let v: OtherType? = nil\n}\n",
        }
        self.assertFails(files, path, "uses 'OtherType'")


class TheLexer(Base):
    """What the comment and string stripping must and must not hide from the rules."""

    def kotlin(self, body: str) -> list[str]:
        return self.found({KOTLIN_SCREEN_PATH: KOTLIN_SCREEN % body})

    def test_a_violation_after_each_kind_of_literal_on_the_same_line_is_still_found(self):
        for before in (
            "val q = '\"'; ",
            "val q = '\\''; ",
            'val s = "a\\"b"; ',
            'val s = """a"b"""; ',
            "/* c */ ",
            'val s = "x"; ',
        ):
            with self.subTest(before=before):
                self.assertTrue(any("size literal" in line for line in self.kotlin(f"    {before}padding(16.dp)")))

    def test_a_violation_after_a_swift_raw_string_is_still_found(self):
        text = SWIFT_SCREEN % '        let s = #"a"b"#; let p = Text().padding(8)'
        self.assertTrue(any("size literal" in line for line in self.found({SWIFT_SCREEN_PATH: text})))

    def test_a_violation_inside_a_string_or_a_comment_is_not_one(self):
        for body in (
            '    val s = "16.dp"',
            "    // padding(16.dp)",
            "    /* padding(16.dp)\n       Color(0xFF) */",
            '    val s = """\n16.dp\n"""',
        ):
            with self.subTest(body=body):
                self.assertEqual(self.kotlin(body), [])

    def test_line_numbers_survive_a_block_comment_and_a_multi_line_string(self):
        body = '    /* one\n       two */\n    val s = """a\nb"""\n    padding(16.dp)'
        self.assertIn("NotesScreen.kt:8:", self.kotlin(body)[0])

    def test_a_balanced_interpolated_quote_does_not_hide_the_rest_of_the_line(self):
        found = self.kotlin('    val s = "${x("y")}"; padding(16.dp)')
        self.assertTrue(any("size literal" in line for line in found), found)

    def test_every_kind_of_character_literal_is_skipped(self):
        for literal in ("'\"'", "'\\\"'", "'\\''", "'a'"):
            with self.subTest(literal=literal):
                found = self.kotlin(f"    val q = {literal}; padding(16.dp)")
                self.assertTrue(any("size literal" in line for line in found), found)

    def test_nested_block_comments_end_where_the_outer_one_ends(self):
        found = self.kotlin("    /* a /* b */ padding(1.dp) */ padding(16.dp)")
        self.assertEqual(len(found), 1, found)
        self.assertIn("'16.dp'", found[0])

    def test_an_unterminated_string_ends_at_the_line_and_the_next_line_is_scanned(self):
        found = self.kotlin('    val s = "oops\n    padding(16.dp)')
        self.assertTrue(any("NotesScreen.kt:5:" in line for line in found), found)

    def test_a_raw_string_with_more_hashes_and_a_backslash_ends_at_its_own_closing(self):
        text = SWIFT_SCREEN % '        let s = ##"a"#b\\"##; let p = Text().padding(8)'
        found = self.found({SWIFT_SCREEN_PATH: text})
        self.assertTrue(any("size literal" in line for line in found), found)

    def test_code_inside_an_interpolation_is_not_scanned(self):
        # A known limit, in the README.
        self.assertEqual(self.kotlin('    val t = "${Color.Red} ${NotesHolder} ${16.dp}"'), [])
        self.assertEqual(
            self.found({SWIFT_SCREEN_PATH: SWIFT_SCREEN % '        Text("\\(FooStore.x) \\(CGFloat(4))")'}), []
        )


if __name__ == "__main__":
    unittest.main()
