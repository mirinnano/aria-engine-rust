export class WebAudioAdapter {
  constructor(readAsset = null) {
    this.readAsset = readAsset;
    this.context = null;
    this.master = null;
    this.buses = new Map();
    this.playing = new Map();
    this.idleSuspendTimer = null;
  }

  installUnlock(target) {
    const unlock = () => {
      this.ensureContext();
      // Keep the first user gesture as the browser's audio permission
      // boundary, but do not leave WebKit's real-time audio renderer running
      // after a title/menu click when no sound has been requested.  WebKitGTK
      // otherwise keeps several audio threads awake on an entirely silent
      // screen, which is both noticeable CPU use and needless battery drain.
      void this.context.resume()
        .then(() => this.suspendWhenSilent())
        .catch(() => {
          // A denied gesture simply leaves the context suspended. A later
          // legitimate playback command still follows the normal error path.
        });
      target.removeEventListener("pointerdown", unlock, true);
      target.removeEventListener("keydown", unlock, true);
    };
    target.addEventListener("pointerdown", unlock, true);
    target.addEventListener("keydown", unlock, true);
  }

  ensureContext() {
    if (this.context) return;
    this.context = new AudioContext();
    this.master = this.context.createGain();
    this.master.connect(this.context.destination);
    for (const bus of ["bgm", "sound_effect", "voice"]) {
      const gain = this.context.createGain();
      gain.connect(this.master);
      this.buses.set(bus, gain);
    }
  }

  suspendWhenSilent(delay = 0) {
    if (!this.context || this.playing.size > 0) return;
    if (this.idleSuspendTimer !== null) clearTimeout(this.idleSuspendTimer);
    this.idleSuspendTimer = setTimeout(() => {
      this.idleSuspendTimer = null;
      if (!this.context || this.playing.size > 0 || this.context.state !== "running") return;
      // Suspending retains the already user-unlocked context, so a later BGM
      // command can resume without asking the player for another gesture.
      void this.context.suspend();
    }, delay);
  }

  async consume(commands) {
    if (!commands.length) return;
    this.ensureContext();
    if (this.idleSuspendTimer !== null) {
      clearTimeout(this.idleSuspendTimer);
      this.idleSuspendTimer = null;
    }
    if (this.context.state !== "running") await this.context.resume();
    for (const command of commands) {
      if (command.kind === "play") await this.play(command);
      else if (command.kind === "stop") this.stop(command);
      else if (command.kind === "set_bus_volume") this.setBusVolume(command);
    }
    this.suspendWhenSilent();
  }

  // Save restoration rebuilds the VM's complete audio semantic state on its
  // next frame. Drop every device-local source first so a track that existed
  // only in the pre-restore state cannot continue underneath the restored
  // BGM/SE/voice set. Do not create an AudioContext merely to clear an empty
  // or not-yet-unlocked adapter.
  stopAll() {
    if (!this.context) {
      this.playing.clear();
      return;
    }
    for (const sound of this.playing.values()) {
      try {
        sound.source.stop();
      } catch {
        // A source may already have ended between the iteration and stop().
        // It is still safe to forget it as part of the restore boundary.
      }
    }
    this.playing.clear();
    this.suspendWhenSilent();
  }

  async play(command) {
    let encoded;
    if (this.readAsset) {
      encoded = this.readAsset(command.asset);
    } else {
      const response = await fetch(command.asset);
      if (!response.ok) throw new Error(`audio ${command.asset}: HTTP ${response.status}`);
      encoded = await response.arrayBuffer();
    }
    const bytes = encoded instanceof Uint8Array ? encoded.buffer.slice(
      encoded.byteOffset,
      encoded.byteOffset + encoded.byteLength,
    ) : encoded;
    const buffer = await this.context.decodeAudioData(bytes);
    const source = this.context.createBufferSource();
    const gain = this.context.createGain();
    source.buffer = buffer;
    source.loop = command.looping;
    source.connect(gain);
    gain.connect(this.buses.get(command.bus));
    const now = this.context.currentTime;
    const duration = command.fade_in_ms / 1000;
    gain.gain.setValueAtTime(duration ? 0 : command.volume, now);
    if (duration) gain.gain.linearRampToValueAtTime(command.volume, now + duration);
    const key = `${command.bus}:${command.id}`;
    this.playing.get(key)?.source.stop();
    this.playing.set(key, { source, gain, bus: command.bus, id: command.id });
    source.addEventListener("ended", () => {
      // A stopped pre-restore source can end after its restored replacement
      // was inserted under the same key. Never let that stale callback erase
      // the replacement from the active-track map.
      if (this.playing.get(key)?.source === source) this.playing.delete(key);
      this.suspendWhenSilent();
    }, { once: true });
    source.start();
  }

  stop(command) {
    let longestFade = 0;
    for (const [key, sound] of this.playing) {
      if (sound.bus !== command.bus || (command.id && sound.id !== command.id)) continue;
      const now = this.context.currentTime;
      const duration = command.fade_out_ms / 1000;
      longestFade = Math.max(longestFade, duration * 1000);
      if (duration) {
        sound.gain.gain.cancelScheduledValues(now);
        sound.gain.gain.setValueAtTime(sound.gain.gain.value, now);
        sound.gain.gain.linearRampToValueAtTime(0, now + duration);
        sound.source.stop(now + duration);
      } else {
        sound.source.stop();
      }
      this.playing.delete(key);
    }
    this.suspendWhenSilent(longestFade);
  }

  setBusVolume(command) {
    const gain = this.buses.get(command.bus)?.gain;
    if (!gain) return;
    const now = this.context.currentTime;
    gain.cancelScheduledValues(now);
    gain.setValueAtTime(gain.value, now);
    if (command.fade_ms) {
      gain.linearRampToValueAtTime(command.volume, now + command.fade_ms / 1000);
    } else {
      gain.setValueAtTime(command.volume, now);
    }
  }
}
