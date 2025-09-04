#!/usr/bin/env bash

# This file is part of midnight-ledger.
# Copyright (C) 2025 Midnight Foundation
# SPDX-License-Identifier: Apache-2.0
# Licensed under the Apache License, Version 2.0 (the "License");
# You may not use this file except in compliance with the License.
# You may obtain a copy of the License at
# http://www.apache.org/licenses/LICENSE-2.0
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

find . -type f -name "*.rs" \( -path "*/build.rs" -o -path "*/src/*" -o -path "*/tests/*" \) | while IFS= read -r file; do
if ! grep -q "SPDX-License-Identifier" "$file"; then
	echo "No license info: $file"
	exit 1
fi
done

find . -type f \( -name "*.js" -o -name "*.ts" \) \( -path "*/src/*" -o -path "*/tests/*" \) | while IFS= read -r file; do
if ! grep -q "SPDX-License-Identifier" "$file"; then
	echo "No license info: $file"
	exit 1
fi
done

find . -type f -name "*.sh" | while IFS= read -r file; do
if ! grep -q "SPDX-License-Identifier" "$file"; then
	echo "No license info: $file"
	exit 1
fi
done

find . -type f -name "*.cjs" -not -path "*/.yarn/*" | while IFS= read -r file; do
if ! grep -q "SPDX-License-Identifier" "$file"; then
	echo "No license info: $file"
	exit 1
fi
done

find . -type f -name "*.compact" | while IFS= read -r file; do
if ! grep -q "SPDX-License-Identifier" "$file"; then
	echo "No license info: $file"
	exit 1
fi
done

find . -type f -name "*.mjs" | while IFS= read -r file; do
if ! grep -q "SPDX-License-Identifier" "$file"; then
	echo "No license info: $file"
	exit 1
fi
done

find . -type f -name "*.nix" | while IFS= read -r file; do
if ! grep -q "SPDX-License-Identifier" "$file"; then
	echo "No license info: $file"
	exit 1
fi
done

find . -type f -name "*.py" | while IFS= read -r file; do
if ! grep -q "SPDX-License-Identifier" "$file"; then
	echo "No license info: $file"
	exit 1
fi
done

find . -type f -name "*.ss" | while IFS= read -r file; do
if ! grep -q "SPDX-License-Identifier" "$file"; then
	echo "No license info: $file"
	exit 1
fi
done
