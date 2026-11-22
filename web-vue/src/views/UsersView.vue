<script setup lang="ts">
// Users & invites (owner): add a local user (a home profile, perhaps with a PIN), invite someone by a link
// (mailed by the server when it can), and the people with what each may do: libraries, admin, converting,
// deleting files, downloading, parental controls, home profile and PIN, a new password, removing them.
import { onMounted, reactive, ref } from 'vue';
import { api, get, session } from '../api';
import { lang, t } from '../i18n';
import AskDialog from '../components/AskDialog.vue';
import Modal from '../components/Modal.vue';

interface User {
  id: number, username: string, display_name?: string, email?: string, admin: boolean, two_factor?: boolean, created_at?: number,
  allowed_sections: number[] | null, allow_transcode: boolean, allow_delete: boolean, allow_download: boolean, max_age: number | null,
  block_unrated: boolean, home: boolean, has_pin: boolean,
}
interface Invite { code: string, email?: string, note?: string, status: 'open' | 'used' | 'expired', used_by?: string, expires_at?: number, allowed_sections: number[] | null, url: string }

const users = ref<User[] | null>(null);
const invites = ref<Invite[]>([]);
const error = ref('');
const note = ref('');
let noteTimer = 0;
function say(text: string) { note.value = text; clearTimeout(noteTimer); noteTimer = window.setTimeout(() => { note.value = ''; }, 4000); }
const mail = !!(session.status && session.status.mail);
const allIds = () => session.sections.map(s => s.id);

async function load() {
  try {
    const [u, i] = await Promise.all([get<{ users: User[] }>('/api/users'), get<{ invites: Invite[] }>('/api/invites')]);
    users.value = u.users; invites.value = i.invites;
  } catch (e) { error.value = (e as Error).message; }
}
onMounted(load);
const secName = (id: number) => session.sections.find(s => s.id === id)?.title || '#' + id;
const shown = (u: User) => u.display_name || u.username;
const date = (s?: number) => (s ? new Date(s * 1000).toLocaleDateString(lang.value) : '');

// ---- a local user
const local = reactive({ username: '', password: '', home: false, pin: '', admin: false, all: true, secs: allIds(), error: '' });
async function addLocal() {
  local.error = '';
  const body: Record<string, unknown> = { username: local.username, password: local.password, admin: local.admin, home: local.home, pin: local.pin };
  if (!local.all) body.allowed_sections = local.secs;
  try {
    await api('POST', '/api/users', body);
    say(t('{name} was added', { name: local.username.trim() }));
    Object.assign(local, { username: '', password: '', home: false, pin: '', admin: false, all: true, secs: allIds() });
    load();
  } catch (e) { local.error = (e as Error).message; }
}

// ---- an invite
const inv = reactive({ email: '', note: '', days: 7, admin: false, byMail: mail, all: true, secs: allIds() });
const made = ref<{ url: string, email?: string, note?: string, mailed?: boolean, mail_error?: string } | null>(null);
async function invite() {
  const body: Record<string, unknown> = { email: inv.email, note: inv.note, days: Number(inv.days) || 7, admin: inv.admin, send_mail: mail && inv.byMail && inv.email.trim() !== '', lang: lang.value };
  if (!inv.all) body.allowed_sections = inv.secs;
  try { made.value = await api('POST', '/api/invites', body); load(); } catch (e) { say((e as Error).message); }
}
const mailto = (r: { email?: string, note?: string, url: string }) => `mailto:${encodeURIComponent(r.email || '')}?subject=${encodeURIComponent(t('Invitation to {server}', { server: session.status?.name || 'Lichtspiel' }))}&body=${encodeURIComponent((r.note ? r.note + '\n\n' : '') + t('Join here: {url}', { url: r.url }))}`;
function copy(url: string) { navigator.clipboard?.writeText(url).catch(() => {}); say(t('Link copied')); }

// ---- changes to a person (PATCH /api/users/{id})
async function patch(u: User, body: Record<string, unknown>, reload = false) {
  try { await api('PATCH', '/api/users/' + u.id, body); say(t('Saved')); } catch (e) { say((e as Error).message); reload = true; }
  if (reload) load();
}
const access = ref<{ u: User, all: boolean, secs: number[] } | null>(null);
function saveAccess() {
  const a = access.value; if (!a) return;
  access.value = null;
  patch(a.u, { allowed_sections: a.all ? null : a.secs }, true);
}
const asking = ref<{ kind: 'pin' | 'password', u: User } | null>(null);
async function answered(v: string | null) {
  const a = asking.value; asking.value = null;
  if (!a || v === null) return;
  if (a.kind === 'pin') { await patch(a.u, { pin: v }, true); say(v ? t('PIN set') : t('PIN removed')); }
  else { await patch(a.u, { password: v }); say(t('Password changed')); }
}
const removing = ref<User | null>(null);
const typed = ref('');
async function remove() {
  const u = removing.value; removing.value = null;
  if (!u) return;
  try { await api('DELETE', '/api/users/' + u.id); load(); } catch (e) { say((e as Error).message); }
}
const resetting = ref<User | null>(null);
async function reset2fa() { const u = resetting.value; resetting.value = null; if (u) await patch(u, { reset_2fa: true }, true); }
const dropping = ref<Invite | null>(null);
async function dropInvite() {
  const i = dropping.value; dropping.value = null;
  if (!i) return;
  try { await api('DELETE', '/api/invites/' + i.code); load(); } catch (e) { say((e as Error).message); }
}
const checked = (e: Event) => (e.target as HTMLInputElement).checked;
</script>

<template>
  <div>
    <h1>{{ t('Users & invites') }}</h1>
    <p v-if="error" class="err">{{ error }}</p>
    <div v-if="note" class="toast">{{ note }}</div>

    <section class="panel">
      <h2>{{ t('Add a local user') }}</h2>
      <p class="dim">{{ t('For people in your home: you pick their user name and password, and they sign in with it right away. No invite link needed.') }}</p>
      <div class="form">
        <label for="u-name">{{ t('User name') }}</label><input id="u-name" v-model="local.username" autocomplete="off">
        <label for="u-pw">{{ t('Password (at least 6 characters)') }}</label>
        <div><input id="u-pw" v-model="local.password" type="password" autocomplete="new-password"><p class="dim hint">{{ t('May stay empty for a home profile: it is then only picked on the sign-in screen.') }}</p></div>
        <label for="u-home">{{ t('Home profile') }}</label>
        <div class="val"><input id="u-home" v-model="local.home" type="checkbox"><p class="dim hint">{{ t('Shown on the sign-in screen of browsers in your home network; a tap signs in.') }}</p></div>
        <label for="u-pin">{{ t('PIN (optional)') }}</label>
        <div><input id="u-pin" v-model="local.pin" type="password" inputmode="numeric" maxlength="8" autocomplete="off" class="short"><p class="dim hint">{{ t('4 to 8 digits, asked when the profile is picked. Admins need one.') }}</p></div>
        <label>{{ t('Libraries they may watch') }}</label>
        <div class="checks">
          <label><input v-model="local.all" type="checkbox"> <b>{{ t('All, including future ones') }}</b></label>
          <label v-for="s in session.sections" :key="s.id"><input v-model="local.secs" type="checkbox" :value="s.id" :disabled="local.all"> {{ s.title }}</label>
        </div>
        <label for="u-admin">{{ t('Make them an admin') }}</label><div class="val"><input id="u-admin" v-model="local.admin" type="checkbox"></div>
      </div>
      <div class="actions"><button class="primary" @click="addLocal">{{ t('Add user') }}</button></div>
      <p v-if="local.error" class="err">{{ local.error }}</p>
    </section>

    <section class="panel">
      <h2>{{ t('Invite someone') }}</h2>
      <p class="dim">{{ t('You get a link to send them (by email, chat …). They open it, pick a user name and password, and can watch the libraries you tick.') }}</p>
      <div class="form">
        <label for="i-mail">{{ t('Their email') }}</label><input id="i-mail" v-model="inv.email" type="email" :placeholder="t('friend@example.com')">
        <label for="i-note">{{ t('Message') }}</label><input id="i-note" v-model="inv.note" :placeholder="t('A short message (optional)')">
        <label for="i-send">{{ t('Send by e-mail') }}</label>
        <div v-if="mail" class="val"><input id="i-send" v-model="inv.byMail" type="checkbox"><p class="dim hint">{{ t('The server mails them the link and your message.') }}</p></div>
        <div v-else class="val dim">{{ t('Needs a mail server: ') }}<a href="/#/settings/general">{{ t('Settings → General') }}</a></div>
        <label for="i-days">{{ t('Link works for (days)') }}</label><input id="i-days" v-model.number="inv.days" type="number" min="1" max="365" class="short">
        <label>{{ t('Libraries they may watch') }}</label>
        <div class="checks">
          <label><input v-model="inv.all" type="checkbox"> <b>{{ t('All, including future ones') }}</b></label>
          <label v-for="s in session.sections" :key="s.id"><input v-model="inv.secs" type="checkbox" :value="s.id" :disabled="inv.all"> {{ s.title }}</label>
        </div>
        <label for="i-admin">{{ t('Make them an admin') }}</label><div class="val"><input id="i-admin" v-model="inv.admin" type="checkbox"></div>
      </div>
      <div class="actions"><button class="primary" @click="invite">{{ t('Create invite link') }}</button></div>
      <div v-if="made" class="panel inner">
        <p v-if="made.mailed" class="ok">{{ t('The invite was sent to {email}. The link, in case you need it:', { email: made.email || '' }) }}</p>
        <template v-else-if="made.mail_error"><p class="err">{{ t(made.mail_error) }}</p><p>{{ t('Send them this link yourself:') }}</p></template>
        <p v-else class="ok">{{ t('Invite ready. Send this link:') }}</p>
        <input :value="made.url" class="full" readonly @click="($event.target as HTMLInputElement).select()">
        <div class="actions"><button @click="copy(made.url)">{{ t('Copy link') }}</button><a v-if="made.email" class="btn" :href="mailto(made)">{{ t('Open in email') }}</a></div>
      </div>
    </section>

    <h2>{{ t('People') }}</h2>
    <div v-if="users === null" class="skel line"></div>
    <div v-else class="table-wrap">
      <table class="table">
        <tr>
          <th>{{ t('User') }}</th><th>{{ t('Libraries') }}</th><th>{{ t('Admin') }}</th><th>{{ t('May convert') }}</th><th>{{ t('May delete files') }}</th>
          <th>{{ t('May download') }}</th><th :title="t('Hides movies and shows rated above this age')">{{ t('Parental controls') }}</th>
          <th :title="t('Shown on the sign-in screen')">{{ t('Home profile') }}</th><th></th>
        </tr>
        <tr v-for="u in users" :key="u.id">
          <td>
            <b>{{ shown(u) }}</b>
            <div v-if="shown(u) !== u.username" class="dim">{{ u.username }}</div>
            <div v-if="u.email" class="dim">{{ u.email }}</div>
            <div v-if="u.two_factor" class="dim small">🔐 {{ t('Two-step sign-in') }} <button class="linkbtn" @click="resetting = u">{{ t('Reset') }}</button></div>
            <div class="dim small">{{ t('joined {date}', { date: date(u.created_at) }) }}</div>
          </td>
          <td>
            {{ u.admin ? t('all (admin)') : u.allowed_sections === null ? t('all') : u.allowed_sections.map(secName).join(', ') || t('none') }}
            <button v-if="!u.admin" class="small" @click="access = { u, all: u.allowed_sections === null, secs: u.allowed_sections ? [...u.allowed_sections] : allIds() }">{{ t('Change') }}</button>
          </td>
          <td><input type="checkbox" :checked="u.admin" :disabled="u.id === session.me?.id" :aria-label="t('Admin')" @change="patch(u, { admin: checked($event) }, true)"></td>
          <td><input type="checkbox" :checked="u.allow_transcode" :title="t('Allow converting video on the fly')" @change="patch(u, { allow_transcode: checked($event) })"></td>
          <td><input type="checkbox" :checked="u.admin || u.allow_delete" :disabled="u.admin"
            :title="u.admin ? t('Admins may always delete files (when media deletion is allowed in the settings)') : t('May delete movie and episode files from disk in the libraries they can see')"
            @change="patch(u, { allow_delete: checked($event) })"></td>
          <td><input type="checkbox" :checked="u.allow_download" :disabled="u.admin"
            :title="u.admin ? t('Admins may always download') : t('May download movies and episodes, as they are or converted to a smaller quality')"
            @change="patch(u, { allow_download: checked($event) })"></td>
          <td>
            <span v-if="u.admin" class="dim">–</span>
            <template v-else>
              <select :value="u.max_age ?? ''" @change="patch(u, { max_age: ($event.target as HTMLSelectElement).value === '' ? null : Number(($event.target as HTMLSelectElement).value) }, true)">
                <option value="">{{ t('No limit') }}</option>
                <option v-for="a in [0, 6, 12, 16, 18]" :key="a" :value="a">{{ t('up to {age}', { age: a }) }}</option>
              </select>
              <label v-if="u.max_age !== null" class="dim small block" :title="t('Also hide titles without an age rating')">
                <input type="checkbox" :checked="u.block_unrated" @change="patch(u, { block_unrated: checked($event) })"> {{ t('Hide unrated') }}
              </label>
            </template>
          </td>
          <td class="nowrap">
            <input type="checkbox" :checked="u.home" :title="t('Shown on the sign-in screen')" @change="patch(u, { home: checked($event) }, true)">
            <button class="small" :title="u.has_pin ? t('PIN is set') : t('No PIN')" @click="asking = { kind: 'pin', u }">{{ t('PIN') }}{{ u.has_pin ? ' 🔒' : '' }}</button>
          </td>
          <td class="nowrap">
            <span v-if="u.id === session.me?.id" class="dim">{{ t('you') }}</span>
            <template v-else>
              <button class="small" @click="asking = { kind: 'password', u }">{{ t('Password') }}</button>
              <button class="small danger" @click="removing = u; typed = ''">{{ t('Remove') }}</button>
            </template>
          </td>
        </tr>
      </table>
    </div>

    <h2>{{ t('Invites') }}</h2>
    <p v-if="!invites.length" class="dim">{{ t('No invites yet.') }}</p>
    <div v-else class="table-wrap">
      <table class="table">
        <tr><th>{{ t('For') }}</th><th>{{ t('Status') }}</th><th>{{ t('Libraries') }}</th><th>{{ t('Link') }}</th><th></th></tr>
        <tr v-for="i in invites" :key="i.code">
          <td>{{ i.email || '' }}<span v-if="!i.email" class="dim">{{ t('anyone with the link') }}</span><div v-if="i.note" class="dim">{{ i.note }}</div></td>
          <td>{{ i.status === 'used' ? t('used by {name}', { name: i.used_by || '' }) : i.status === 'open' ? t('open until {date}', { date: date(i.expires_at) }) : t('expired') }}</td>
          <td>{{ i.allowed_sections ? i.allowed_sections.map(secName).join(', ') : t('all') }}</td>
          <td><button v-if="i.status === 'open'" class="small" @click="copy(i.url)">{{ t('Copy link') }}</button></td>
          <td><button class="small danger" @click="dropping = i">{{ i.status === 'open' ? t('Cancel') : t('Delete') }}</button></td>
        </tr>
      </table>
    </div>

    <Modal v-if="access" :title="t('Libraries for {name}', { name: access.u.username })" @close="access = null">
      <div class="checks">
        <label><input v-model="access.all" type="checkbox"> <b>{{ t('All, including future ones') }}</b></label>
        <label v-for="s in session.sections" :key="s.id"><input v-model="access.secs" type="checkbox" :value="s.id" :disabled="access.all"> {{ s.title }}</label>
      </div>
      <template #actions><button class="primary" @click="saveAccess">{{ t('Save') }}</button><button @click="access = null">{{ t('Cancel') }}</button></template>
    </Modal>
    <AskDialog v-if="asking" :title="asking.kind === 'pin' ? t('PIN for {name}', { name: asking.u.username }) : t('New password for {name}', { name: asking.u.username })"
      password :allow-empty="asking.kind === 'pin'" :hint="asking.kind === 'pin' ? t('4 to 8 digits. Leave empty to remove the PIN.') : ''" @done="answered" />
    <Modal v-if="removing" :title="t('Remove {name}?', { name: removing.username })" @close="removing = null">
      <p class="dim">{{ t('Type “{name}” to confirm.', { name: removing.username }) }}</p>
      <input v-model="typed" class="full" autocomplete="off">
      <template #actions><button class="danger" :disabled="typed !== removing.username" @click="remove">{{ t('Remove') }}</button><button @click="removing = null">{{ t('Cancel') }}</button></template>
    </Modal>
    <Modal v-if="resetting" :title="t('Switch off the two-step sign-in of {name}?', { name: resetting.username })" @close="resetting = null">
      <template #actions><button class="danger" @click="reset2fa">{{ t('Switch off') }}</button><button @click="resetting = null">{{ t('Cancel') }}</button></template>
    </Modal>
    <Modal v-if="dropping" :title="dropping.status === 'open' ? t('Cancel this invite?') : t('Delete this invite?')" @close="dropping = null">
      <template #actions><button class="danger" @click="dropInvite">{{ dropping.status === 'open' ? t('Cancel') : t('Delete') }}</button><button @click="dropping = null">{{ t('Back') }}</button></template>
    </Modal>
  </div>
</template>
