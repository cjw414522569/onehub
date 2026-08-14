plugins {
    base
}

val contract = layout.projectDirectory.file("contract.json")

fun contractTask(name: String, expected: String) = tasks.register(name) {
    doLast {
        check(contract.asFile.exists()) { "Missing Android boundary contract" }
        val text = contract.asFile.readText()
        check(text.contains("\"status\": \"interface-only\"")) { "Android boundary must remain interface-only" }
        check(text.contains("\"approved_bridge\": \"bindings-kotlin\"")) { "Android bridge must remain bindings-kotlin" }
        println("$expected: Android interface boundary contract verified")
    }
}

contractTask("platformBuild", "build")
contractTask("platformLint", "lint")
contractTask("platformTest", "unit-test")
