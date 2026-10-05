// Copyright 2023 Mozilla Foundation
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import * as core from '@actions/core';
import {context} from '@actions/github';
import * as exec from '@actions/exec';
import {SummaryTableRow} from '@actions/core/lib/summary';

async function get_output(command: string, args: string[]): Promise<string> {
  core.debug(`get_output: ${command} ${args.join(' ')}`);

  const output = await exec.getExecOutput(command, args);
  // --stats-format=json does not emit a newline, which messes up group
  // processing. Here we add a newline if it's missing.
  if (!output.stdout.endsWith('\n')) {
    process.stdout.write('\n');
  }
  return output.stdout.toString();
}

async function show_stats() {
  const disable_annotations = core.getBooleanInput('disable_annotations');
  if (disable_annotations) {
    core.debug('annotations generation disabled');
    return;
  }

  core.debug('start sccache show starts');
  const human_stats = await core.group('Get human-readable stats', async () => {
    return get_output(`${process.env.SCCACHE_PATH}`, ['--show-stats']);
  });
  const json_stats = await core.group('Get JSON stats', async () => {
    return get_output(`${process.env.SCCACHE_PATH}`, [
      '--show-stats',
      '--stats-format=json'
    ]);
  });

  const stats: Stats = JSON.parse(json_stats);
  const formatted_stats = format_json_stats(stats);

  core.notice(formatted_stats.notice, {
    title: `sccache stats - ${context.job}`
  });

  core.info('\nFull human-readable stats:');
  core.info(human_stats);

  core.summary.addHeading('sccache stats', 2);
  core.summary.addTable(formatted_stats.table);
  core.summary.addDetails(
    'Full human-readable stats',
    '\n\n```\n' + human_stats + '\n```\n\n'
  );
  core.summary.addDetails(
    'Full JSON Stats',
    '\n\n```json\n' + JSON.stringify(stats, null, 2) + '\n```\n\n'
  );

  await core.summary.write();
}

show_stats().catch(err => {
  core.error(err);
  core.setFailed(err.message);
});

interface Duration {
  secs: number;
  nanos: number;
}

interface Counter {
  counts: {
    [key: string]: number;
  };
  adv_counts: {
    [key: string]: number;
  };
}

interface Stats {
  stats: {
    compile_requests: number;
    requests_executed: number;

    cache_errors: Counter;
    cache_hits: Counter;
    cache_misses: Counter;

    cache_write_errors: number;
    cache_writes: number;

    cache_write_duration: Duration;
    cache_read_hit_duration: Duration;
    compiler_write_duration: Duration;
  };
}

function sum_stats(stats: Counter): number {
  return Object.values(stats.counts).reduce((acc, val) => acc + val, 0);
}

function format_duration(duration: Duration): string {
  const ms = duration.nanos / 1e6;
  return `${duration.secs}s ${ms}ms`;
}

function format_json_stats(stats: Stats): {
  table: SummaryTableRow[];
  notice: string;
} {
  const cache_error_count = sum_stats(stats.stats.cache_errors);
  const cache_hit_count = sum_stats(stats.stats.cache_hits);
  const cache_miss_count = sum_stats(stats.stats.cache_misses);
  const total_hits = cache_hit_count + cache_miss_count + cache_error_count;
  const ratio = percentage(cache_hit_count, total_hits);

  const write_duration = format_duration(stats.stats.cache_write_duration);
  const read_duration = format_duration(stats.stats.cache_read_hit_duration);
  const compiler_duration = format_duration(
    stats.stats.compiler_write_duration
  );

  const notice_hit = plural(cache_hit_count, 'hit');
  const notice_miss = plural(cache_miss_count, 'miss', 'misses');
  const notice_error = plural(cache_error_count, 'error');
  const notice = `${ratio}% - ${notice_hit}, ${notice_miss}, ${notice_error}`;

  const table = [
    [{data: 'Cache hit %', header: true}, {data: `${ratio}%`}],
    [{data: 'Cache hits', header: true}, {data: cache_hit_count.toString()}],
    [{data: 'Cache misses', header: true}, {data: cache_miss_count.toString()}],
    [
      {data: 'Cache errors', header: true},
      {data: cache_error_count.toString()}
    ],
    [
      {data: 'Compile requests', header: true},
      {data: stats.stats.compile_requests.toString()}
    ],
    [
      {data: 'Requests executed', header: true},
      {data: stats.stats.requests_executed.toString()}
    ],
    [
      {data: 'Cache writes', header: true},
      {data: stats.stats.cache_writes.toString()}
    ],
    [
      {data: 'Cache write errors', header: true},
      {data: stats.stats.cache_write_errors.toString()}
    ],
    [{data: 'Cache write duration', header: true}, {data: write_duration}],
    [{data: 'Cache read hit duration', header: true}, {data: read_duration}],
    [{data: 'Compiler write duration', header: true}, {data: compiler_duration}]
  ];
  return {table, notice};
}

function percentage(x: number, y: number): number {
  return Math.round((x / y) * 100 || 0);
}

function plural(count: number, base: string, plural = base + 's'): string {
  return `${count} ${count === 1 ? base : plural}`;
}
