package cf.construct.mobile.core.chat

/** The text of a file the tests compare with; `name` is a path from the repository root. */
fun fixtureText(name: String): String = GeneratedFixtures.files.getValue(name)
