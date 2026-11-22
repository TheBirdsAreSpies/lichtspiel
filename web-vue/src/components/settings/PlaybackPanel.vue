<script setup lang="ts">
// Settings → Playback: quality, direct play, the preferred audio and subtitle languages, how subtitles
// look, and on which posters the watched mark shows. Saved with the account (POST /api/me/prefs) under the classic app's names, so both apps (and every
// browser) use the same settings. Each choice is saved at once.
import { ref } from 'vue';
import { pref, savePref } from '../../api';
import { fmtNum, langName } from '../../format';
import { t } from '../../i18n';

type Opt = [string, string];
interface Row { key: string, def: string, label: string, opts: Opt[], hint?: string }

const LANGS: Opt[] = [['', 'Not set'], ...['en', 'de', 'fr', 'es', 'it', 'pt', 'nl', 'sv', 'da', 'no', 'fi', 'pl', 'ru', 'tr', 'ja', 'ko', 'zh'].map(c => [c, ''] as Opt)];
const langLabel = (o: Opt) => (o[0] ? langName(o[0]) : t(o[1]));
const QUALITIES: Opt[] = [['original', 'Original quality'], ['1080p-20', '1080p 20 Mbps'], ['1080p-12', '1080p 12 Mbps'], ['1080p-8', '1080p 8 Mbps'],
  ['720p-4', '720p 4 Mbps'], ['720p-3', '720p 3 Mbps'], ['720p-2', '720p 2 Mbps'], ['480p-1.5', `480p ${fmtNum(1.5)} Mbps`], ['320p-0.7', `320p ${fmtNum(0.7)} Mbps`]];

const QUALITY: Row[] = [
  { key: 'quality', def: 'original', label: 'Video quality', opts: QUALITIES, hint: 'The quality used when a video starts. Saved with your account.' },
  { key: 'keepSmall', def: '1', label: 'Play smaller videos in original quality', opts: [['1', 'Yes'], ['0', 'No']], hint: 'A video that is not bigger than the chosen quality is not converted.' },
  { key: 'directPlay', def: '1', label: 'Direct play', opts: [['1', 'Allowed'], ['0', 'Always convert']], hint: 'Direct play sends the file as it is. Turn it off to always have the server convert, e.g. when a browser plays a file badly.' },
];
const MARK: Row[] = [
  { key: 'watchedMark', def: 'all', label: 'Show the watched mark on', opts: [['all', 'Movies and shows'], ['movies', 'Movies'], ['shows', 'Shows'], ['none', 'Nothing']],
    hint: 'The check mark and dimming on posters of personal media. Saved with your account.' },
];
const SUBS: Row[] = [
  { key: 'subPrefLang', def: '', label: 'Preferred subtitle language', opts: LANGS },
  { key: 'subMode', def: 'manual', label: 'Subtitle mode', opts: [['manual', 'Manually selected'], ['foreign', 'Shown with foreign audio'], ['always', 'Always on']],
    hint: '“Shown with foreign audio” turns subtitles on when the audio is not in your preferred subtitle language.' },
  { key: 'subSdh', def: 'any', label: 'Subtitles for the hard of hearing (SDH)', opts: [['any', 'No preference'], ['prefer', 'Prefer them'], ['avoid', 'Avoid them']],
    hint: 'SDH subtitles also describe sounds. Needs a library that was read again after this version.' },
  { key: 'subForced', def: '1', label: 'Forced subtitles', opts: [['1', 'Show with audio in the preferred language'], ['0', 'Off']],
    hint: 'Forced subtitles only translate foreign lines, e.g. when characters speak another language.' },
  { key: 'burnSubs', def: 'auto', label: 'Burn in subtitles', opts: [['auto', 'Only picture-based (Blu-ray, DVD)'], ['never', 'Never']],
    hint: 'Picture-based subtitles can’t be shown as text, so the server paints them into the video (the video is converted for that).' },
  { key: 'subSize', def: '100', label: 'Subtitle size', opts: [['75', 'Small'], ['100', 'Normal'], ['130', 'Large'], ['170', 'Very large']] },
  { key: 'subBg', def: '1', label: 'Subtitle background', opts: [['1', 'Dark box'], ['0', 'None']] },
  { key: 'subLift', def: '0', label: 'Subtitle position', opts: [['0', 'Bottom'], ['2', 'Slightly higher'], ['4', 'Higher'], ['7', 'Much higher']] },
];

const saved = ref('');
const error = ref('');
let timer = 0;
async function set(key: string, value: string) {
  error.value = '';
  try {
    await savePref(key, value);
    saved.value = key;
    clearTimeout(timer); timer = window.setTimeout(() => { saved.value = ''; }, 1500);
  } catch (e) { error.value = (e as Error).message; }
}
const value = (r: Row) => pref(r.key, r.def);
const color = ref(pref('subColor', '#ffffff'));
</script>

<template>
  <p v-if="error" class="err">{{ error }}</p>
  <section class="panel">
    <h2>{{ t('Quality') }}</h2>
    <div class="form">
      <template v-for="r in QUALITY" :key="r.key">
        <label :for="'p-' + r.key">{{ t(r.label) }}</label>
        <div>
          <select :id="'p-' + r.key" :value="value(r)" @change="set(r.key, ($event.target as HTMLSelectElement).value)">
            <option v-for="o in r.opts" :key="o[0]" :value="o[0]">{{ t(o[1]) }}</option>
          </select>
          <span v-if="saved === r.key" class="ok-msg saved">{{ t('Saved') }}</span>
          <p v-if="r.hint" class="dim hint">{{ t(r.hint) }}</p>
        </div>
      </template>
    </div>
  </section>
  <section class="panel">
    <h2>{{ t('Watched indicator') }}</h2>
    <div class="form">
      <template v-for="r in MARK" :key="r.key">
        <label :for="'p-' + r.key">{{ t(r.label) }}</label>
        <div>
          <select :id="'p-' + r.key" :value="value(r)" @change="set(r.key, ($event.target as HTMLSelectElement).value)">
            <option v-for="o in r.opts" :key="o[0]" :value="o[0]">{{ t(o[1]) }}</option>
          </select>
          <span v-if="saved === r.key" class="ok-msg saved">{{ t('Saved') }}</span>
          <p v-if="r.hint" class="dim hint">{{ t(r.hint) }}</p>
        </div>
      </template>
    </div>
  </section>
  <section class="panel">
    <h2>{{ t('Audio and subtitles') }}</h2>
    <div class="form">
      <label for="p-audioLang">{{ t('Preferred audio language') }}</label>
      <div>
        <select id="p-audioLang" :value="pref('audioLang')" @change="set('audioLang', ($event.target as HTMLSelectElement).value)">
          <option v-for="o in LANGS" :key="o[0]" :value="o[0]">{{ langLabel(o) }}</option>
        </select>
        <span v-if="saved === 'audioLang'" class="ok-msg saved">{{ t('Saved') }}</span>
        <p class="dim hint">{{ t('Used when a video has that audio track. Saved with your account.') }}</p>
      </div>
      <template v-for="r in SUBS" :key="r.key">
        <label :for="'p-' + r.key">{{ t(r.label) }}</label>
        <div>
          <select :id="'p-' + r.key" :value="value(r)" @change="set(r.key, ($event.target as HTMLSelectElement).value)">
            <option v-for="o in r.opts" :key="o[0]" :value="o[0]">{{ r.opts === LANGS ? langLabel(o) : t(o[1]) }}</option>
          </select>
          <span v-if="saved === r.key" class="ok-msg saved">{{ t('Saved') }}</span>
          <p v-if="r.hint" class="dim hint">{{ t(r.hint) }}</p>
        </div>
      </template>
      <label for="p-subColor">{{ t('Subtitle color') }}</label>
      <div><input id="p-subColor" v-model="color" type="color" class="color" @change="set('subColor', color)"><span v-if="saved === 'subColor'" class="ok-msg saved">{{ t('Saved') }}</span></div>
    </div>
    <div class="sub-sample">
      <span :style="{ color, background: pref('subBg', '1') === '1' ? 'rgba(0,0,0,.55)' : 'transparent', fontSize: 22 * Number(pref('subSize', '100')) / 100 + 'px' }">{{ t('This is how subtitles look.') }}</span>
    </div>
  </section>
</template>
