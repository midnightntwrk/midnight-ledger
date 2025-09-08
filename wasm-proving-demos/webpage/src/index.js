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

import * as ProgressBar from 'progressbar.js';
//import init, { initThreadPool, provingProvider } from '@midnight-ntwrk/zkir-v2';

//(async () => {
//  await init();
//  await initThreadPool(navigator.hardwareConcurrency);
//  console.log("a");
//})()

async function get(url) {
  const resp = await fetch(url);
  const blob = await resp.blob();
  return new Uint8Array(await blob.arrayBuffer());
}

let mtTime = undefined;
let stTime = undefined;

const workerMt = new Worker(new URL('./workerMt.js', import.meta.url));
var updateMap = {};
workerMt.onmessage = (m) => {
  console.log(m);
  if(m.data === "init") {
    document.getElementById("prove-button-mt").disabled = false;
  } else {
    if(m.data.msg === "start") {
      updateMap[m.data.id].t0 = new Date();
      updateMap[m.data.id].results.innerHTML = 'Began (multithreaded) proving...';
      updateMap[m.data.id].bar.animate(1.0);
    } else if(m.data.msg === "done") {
      const tn = new Date();
      updateMap[m.data.id].results.innerHTML = `(Multithreaded) proving complete! Took: ${(tn - updateMap[m.data.id].t0) / 1000}s`;
      updateMap[m.data.id].bar.set(1);
      mtTime = tn - updateMap[m.data.id].t0;
    }
  }
};
var ctr = 0;

export async function goProve(useMt) {
  const container = document.getElementById("container");
  const entry = document.createElement("div");
  const progress = document.createElement("div");
  progress.className = "progress";
  const results = document.createElement("span");
  entry.appendChild(progress);
  entry.appendChild(results);
  container.appendChild(entry);
  const last = useMt ? mtTime : stTime;
  const expectedTime = last === undefined ? (useMt ? 10000 : 40000) : last * 1.3;
  console.log(expectedTime)
  var bar = new ProgressBar.Circle(progress, {
    strokeWidth: 35,
    easing: last === undefined ? 'easeOut' : 'linear',
    duration: expectedTime,
    color: '#88f',
    trailColor: '#eee',
    trailWidth: 35,
    svgStyle: null
  });
  if(useMt) {
    const id = ctr++;
    updateMap[id] = {
      t0: undefined,
      results,
      bar,
    };
    workerMt.postMessage(id);
  } else {
    const worker = new Worker(new URL('./worker.js', import.meta.url));
    var t0;
    worker.onmessage = (m) => {
      if(m.data === "start") {
        t0 = new Date();
        results.innerHTML = 'Began (singlethreaded) proving...';
        bar.animate(1.0);
      } else if(m.data === "done") {
        const tn = new Date();
        stTime = tn - t0;
        results.innerHTML = `(Singlethreaded) proving complete! Took: ${(tn - t0) / 1000}s`;
        bar.set(1);
      }
    };
  }
}

window.goProve = goProve;
