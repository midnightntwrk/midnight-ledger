#!/usr/bin/env python3

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

"""
# Overview

This program plots the data gathered by the
midnight_storage::arena::stress_tests::read_write_map_loop test. For example,
you could run

$ cargo run --all-features --release --bin stress -p midnight-storage -- arena::stress_tests::read_write_map_loop 100000 1000

at the top level to generate data in :/tmp, and then run

$ tools/plot.py tmp/read_write_map_loop.100000_1000.<timestamp>.json

to plot the data. Multiple data files are supported: they'll be overlaid on the
same plot, with a different color for each file.

# Install Dependencies

Setup a virtual env and then run

$ pip install pandas seaborn
"""

import json
import pandas as pd
import seaborn as sns
import matplotlib.pyplot as plt
import sys
import math

# Smooth using rolling average
def smooth(series):
    return series.rolling(window=5, center=True).mean()

# Plot smoothed normalized results
def plot(dfs, cols, line_styles, id_var, title):
    fig = plt.figure(figsize=(16, 10))
    ax = fig.add_subplot(111)
    sns.set_theme(style="whitegrid")
    file_colors = sns.color_palette("husl", len(dfs))

    # Smooth each DataFrame
    for df in dfs:
        for col in cols:
            df[col] = smooth(df[col])

    # Normalize and plot data
    for col in cols:
        # Calculate global min/max for this column
        all_values = pd.concat([df[col] for df in dfs])
        col_min = all_values.min()
        col_max = all_values.max()

        for i, df in enumerate(dfs):
            # Normalize using global min/max
            if col_max > col_min:
                normalized = (df[col] - col_min) / (col_max - col_min)
            else:
                normalized = df[col] * 0

            plt.plot(
                df[id_var],
                normalized,
                label=f"{col} (File {i})",
                linestyle=line_styles[col],
                color=file_colors[i],
                linewidth=2
            )

    ax.set_ylabel("Normalized Value [0,1]")
    full_title = f"{title}\n" + "\n".join(f"File {i}: {df.attrs['filename']}" for i, df in enumerate(dfs))
    ax.set_title(full_title, pad=20)
    # Place legend inside the plot in the middle right
    ax.legend(loc='center right', bbox_to_anchor=(0.98, 0.5))
    plt.tight_layout()
    plt.show()

if len(sys.argv) < 2:
    print(f'usage: {sys.argv[0]} FILE1.json [FILE2.json ...]')
    sys.exit(2)

# Load all data files
dfs = []
for filename in sys.argv[1:]:
    with open(filename, "r") as f:
        data = json.load(f)
    df = pd.DataFrame(data["data"])
    df.attrs['filename'] = filename
    dfs.append(df)

# Raw values linear/linear
line_styles = {
    'flush_time': 'solid',
    'cache_size': 'dashed',
    'cache_bytes': 'dotted'
}
cols = ['flush_time', 'cache_size', 'cache_bytes']
plot(dfs, cols, line_styles, id_var='map_size', title='Raw vs map size')

# Raw values linear/log
for df in dfs:
    df['lg_map_size'] = df['map_size'].apply(math.log2)
plot(dfs, cols, line_styles, id_var='lg_map_size', title='Raw vs log2 map size')

# Relative values linear/linear
line_styles = {
    'flush_time_per_cache_size': 'solid',
    'flush_time_per_cache_byte': 'dashed',
    'cache_byte_per_cache_size': 'dotted',
}
for df in dfs:
    df['flush_time_per_cache_size'] = df['flush_time']/df['cache_size']
    df['flush_time_per_cache_byte'] = df['flush_time']/df['cache_bytes']
    df['cache_byte_per_cache_size'] = df['cache_bytes']/df['cache_size']
cols = ['flush_time_per_cache_size', 'flush_time_per_cache_byte', 'cache_byte_per_cache_size']
plot(dfs, cols, line_styles, id_var='map_size', title='Relative vs map size')
