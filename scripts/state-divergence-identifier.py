#!/usr/bin/env python3

import sys
import math
from collections import defaultdict
import re
import difflib

def build_graph(log_path):
    res = defaultdict(list)
    pattern = re.compile('state transition: ([0-9a-f]+) => ([0-9a-f]+)')
    with open(log_path, 'r') as log_file:
        for line in log_file.readlines():
            match = pattern.search(line)
            if match:
                res[match.group(1)].append(match.group(2))
    return res

def longest_path(graph):
    parents = defaultdict(list)
    for node, children in graph.items():
        for child in children:
            parents[child].append(node)
    genesis_points = [key for key in graph.keys() if len(parents[key]) == 0]
    dists = defaultdict(lambda: (math.inf, None))
    for genesis in genesis_points:
        dists[genesis] = (0, None)
    frontier = genesis_points
    while len(frontier) != 0:
        curr = frontier.pop()
        nxt = graph[curr]
        for node in nxt:
            new_dist = dists[curr][0] + 1
            if new_dist < dists[node][0]:
                dists[node] = (new_dist, curr)
                frontier.append(node)
    end = max(dists.items(), key=lambda x: x[1][0])

    curr = end
    path = []
    while curr is not None:
        path.append(curr[0])
        nxt = curr[1][1]
        if nxt:
            curr = (nxt, dists[nxt])
        else:
            curr = None
    return list(reversed(path))

def graph2set(graph):
    nodeset = []
    for key, values in graph.items():
        nodeset.append(key)
        nodeset.extend(values)
    return set(nodeset)

def graph_size(graph):
    return len(graph2set(graph))

if len(sys.argv) != 3:
    print("Usage: scripts/state-divergence-identifier.py <LOGFILE 1> <LOGFILE 2>")
    print("")
    print("This script finds a point where the state evolutions of two logfiles diverge.")
    print("It relies on the ledger 'state transition: <xyz> => <abc>' debug logs,")
    print("building a tree graph for both, finding the longest path in both, and")
    print("finding the point these paths diverge")
else:
    print("parsing log 1...")
    graph1 = build_graph(sys.argv[1])
    print("parsing log 2...")
    graph2 = build_graph(sys.argv[2])
    if graph2set(graph1).isdisjoint(graph2set(graph2)):
        print("graphs are disjoint!")
        print("total entries: {} / {}".format(graph_size(graph1), graph_size(graph2)))
    print("finding longest paths...")
    path1 = longest_path(graph1)
    path2 = longest_path(graph2)
    i, j, k = difflib.SequenceMatcher(a=path1, b=path2).find_longest_match()
    if k == 0:
        print("no common substring on longest paths")
    else:
        print("common substring of length {} found".format(k))
        print("first common state hash: {}".format(path1[i]))
        print("last common state hash: {}".format(path1[i + k - 1]))
        print("substring starts at state #{}, ends at state #{} in log 1's longest path of {} states".format(i, i + k - 1, len(path1)))
        print("substring starts at state #{}, ends at state #{} in log 2's longest path of {} states".format(j, j + k - 1, len(path2)))
