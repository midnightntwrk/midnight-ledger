// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import { defineConfig, globalIgnores } from "eslint/config";
import typescriptEslint from "@typescript-eslint/eslint-plugin";
import _import from "eslint-plugin-import";
import { fixupPluginRules } from "@eslint/compat";
import globals from "globals";
import tsParser from "@typescript-eslint/parser";
import path from "node:path";
import { fileURLToPath } from "node:url";
import js from "@eslint/js";
import { FlatCompat } from "@eslint/eslintrc";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const compat = new FlatCompat({
    baseDirectory: __dirname,
    recommendedConfig: js.configs.recommended,
    allConfig: js.configs.all
});

export default defineConfig([globalIgnores([
    "**/dist",
    "**/.eslintrc.json",
    "**/lib-sources",
    "**/node_modules",
    "**/coverage",
    "**/reports",
    "**/eslint.config.mjs",
    "**/.yarn",
    "**/*.d.ts",
]), {
    extends: compat.extends(
        "plugin:@typescript-eslint/recommended",
        "airbnb-base",
        "plugin:prettier/recommended",
    ),

    plugins: {
        "@typescript-eslint": typescriptEslint,
        import: _import,
    },

    languageOptions: {
        globals: {
            ...globals.browser,
            ...globals.jest,
            logger: "readonly",
        },

        parser: tsParser,
        ecmaVersion: 5,
        sourceType: "module",

        parserOptions: {
            project: ["./tsconfig.json"],
        },
    },

    settings: {
        "import/parsers": {
            "@typescript-eslint/parser": [".ts"],
        },

        "import/resolver": {
            typescript: {
                alwaysTryTypes: false,
                project: ["tsconfig.json"],
            },
        },
    },

    rules: {
        "@typescript-eslint/no-unused-vars": "warn",
        "@typescript-eslint/explicit-member-accessibility": "off",
        "@typescript-eslint/no-object-literal-type-assertion": "off",
        "@typescript-eslint/prefer-interface": "off",
        "@typescript-eslint/camelcase": "off",
        "@typescript-eslint/explicit-function-return-type": "off",
        "@typescript-eslint/no-require-imports": "error",
        "@typescript-eslint/no-use-before-define": "warn",
        "@typescript-eslint/no-shadow": "error",
        "@typescript-eslint/no-explicit-any": "warn",
        "@typescript-eslint/consistent-type-imports": "error",
        "no-shadow": "off",
        "no-use-before-define": "off",
        "import/prefer-default-export": "off",
        "import/no-default-export": "error",
        "import/extensions": "off",
        "import/no-unresolved": "error",
        "import/no-extraneous-dependencies": "off",
        "max-classes-per-file": "off",
        "lines-between-class-members": "off",
        "no-unused-vars": "off",
        "no-underscore-dangle": "off",
        "no-plusplus": "off",
    },
}]);
