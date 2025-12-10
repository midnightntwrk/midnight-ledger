#!/usr/bin/env python3

import sys
import math
from collections import defaultdict
import re

def build_graph(log_path):
    res = defaultdict(list)
    pattern = re.compile('state transition: ([0-9a-f]+) => ([0-9a-f]+)')
    with open(log_path, 'r') as log_file:
        for line in log_file.readlines():
            match = pattern.search(line)
            if match:
                res[match.group(1)].append(match.group(2))
    return res

def longest_path(graph, src):
    dists = defaultdict(lambda: (math.inf, None), {src: (0, None)})
    frontier = [src]
    while len(frontier) != 0:
        curr = frontier.pop()
        next = graph[curr]
        for node in next:
            dists[node] = (dists[curr][0] + 1, curr)
            frontier.append(node)
    end = max(dists.items(), key=lambda x: x[1][0])[0]

    curr = end
    path = []
    while curr:
        path.append(curr)
        curr = dists[curr][1]
    return list(reversed(path))

def find_divergence(path1, path2):
    i = 0
    while i < len(path1) and i < len(path2) and path1[i] == path2[i]:
        i += 1
    if i >= len(path1) or i >= len(path2):
        return None
    else:
        return i

if len(sys.argv) != 4:
    print("Usage: scripts/state-divergence-identifier.py <LOGFILE 1> <LOGFILE 2> <GENESIS HASH>")
    print("")
    print("This script finds a point where the state evolutions of two logfiles diverge.")
    print("It relies on the ledger 'state transition: <xyz> => <abc>' debug logs,")
    print("building a tree graph for both, finding the longest path in both, and")
    print("finding the point these paths diverge")
else:
    hash = sys.argv[3]
    print("parsing log 1...");
    graph1 = build_graph(sys.argv[1])
    print("parsing log 2...");
    graph2 = build_graph(sys.argv[2])
    print("finding longest paths...")
    path1 = longest_path(graph1, hash)
    path2 = longest_path(graph2, hash)
    divergence_point = find_divergence(path1, path2)
    if divergence_point == None and len(path1) != len(path2):
        print("Longest paths do not diverge, but lengths differ: {0} vs {1}".format(len(path1), len(path2)))
    elif divergence_point == None:
        print("Longest paths do not diverge")
    else:
        print("Longest paths diverge at hash {0}:".format(path1[divergence_point - 1]))
        print("Going to {0} and {1} respectively".format(path1[divergence_point], path2[divergence_point]))
        print("Full children at divergence: {0} / {1}".format(graph1[path1[divergence_point - 1]], graph2[path2[divergence_point - 1]]))
