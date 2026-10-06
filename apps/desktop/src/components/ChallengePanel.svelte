<script lang="ts">
  import type { Snapshot } from '../lib/types';
  import { openSource } from '../lib/ipc';
  import Icon from './Icon.svelte';
  let { news, now }: { news: Snapshot['news']; now: number } = $props();
  let error = $state('');
  const challenge = $derived(news.challenge);
  const date = $derived(
    new Intl.DateTimeFormat('en-CA', {
      timeZone: 'America/Los_Angeles',
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
    }).format(now),
  );
  const day = $derived(
    challenge
      ? Math.round((Date.parse(date) - Date.parse(challenge.start_date)) / 86400000) + 1
      : 0,
  );
  const today = $derived(challenge?.records.find((r) => r.day === day));
  async function open(url: string) {
    try {
      await openSource(url);
    } catch (e) {
      error = String(e);
    }
  }
</script>

<section class="cp-challenge" aria-label="Tibo 的 28 天挑战">
  <div class="cp-sectionhead">
    <span>Tibo 的 28 天挑战</span><small
      >{challenge
        ? day < 1
          ? '尚未开始'
          : day > challenge.days
            ? '挑战已结束'
            : `第 ${day} 天 / 共 ${challenge.days} 天`
        : '等待同步'}</small
    >
  </div>
  <p class="cp-note">每天发布一项产品改进，或进行一次完整额度重置。</p>
  {#if challenge}
    <div class="cp-challenge-days" aria-label="28 天每日记录">
      {#each Array.from({ length: challenge.days }, (_, i) => i + 1) as d}{@const record =
          challenge.records.find((r) => r.day === d)}<span
          class:cp-day-kept={!!record?.entries.length}
          class:cp-day-current={d === day}
          title={`第 ${d} 天 · ${record?.entries.map((e) => e.title).join(' / ') || '暂无记录'}`}
          >{d}</span
        >{/each}
    </div>
    <p class="cp-note">
      美西时间 {date}{day >= 1 && day <= challenge.days
        ? ` · 今天：${today?.entries.length ? today.entries.map((e) => (e.kind === 'reset' ? '额度重置' : e.kind === 'improvement' ? '产品改进' : '新动态')).join(' / ') : '等待 Tibo'}`
        : ''}
    </p>
    {#if news.challenge_status !== 'connected'}<p class="cp-note">
        挑战同步暂不可用，保留最近记录。
      </p>{/if}
    {#each challenge.records.filter((r) => r.entries.length) as record}
      <div class="cp-challenge-record">
        <small>第 {record.day} 天 · {record.date}</small>{#each record.entries as entry}<div>
            <span class="cp-tag"
              >{entry.kind === 'reset'
                ? '额度重置'
                : entry.kind === 'improvement'
                  ? '产品改进'
                  : '动态'}</span
            >
            <h3>{entry.title}</h3>
            <p>{entry.text}</p>
            {#if entry.source_url}<button
                class="cp-textbutton"
                onclick={() => void open(entry.source_url!)}
                >查看 Tibo 原帖 <Icon name="external" /></button
              >{/if}
          </div>{/each}
      </div>
    {/each}
  {:else}<p class="cp-note">挑战页面尚未同步；不会把产品改进误算成额度重置。</p>{/if}
  <button class="cp-textbutton" onclick={() => void open('https://codex-resets.com/zh-CN/tibo-28')}
    >查看挑战完整记录 <Icon name="external" /></button
  >
  {#if news.challenge_fetched_at}<small
      >同步于 {new Date(news.challenge_fetched_at).toLocaleString('zh-CN')}</small
    >{/if}
  {#if error}<p class="cp-note" role="status">{error}</p>{/if}
</section>
