import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "../components/Icon";
import { useCompact } from "../components/MobileNav";
import { frenchSpaces } from "../components/Markdown";
import { Orb } from "../components/ui";
import { api, errorText } from "../lib/api";
import { confirmAsk } from "../lib/dialogs";
import { count, locale, t } from "../lib/i18n";
import { langInfo } from "../lib/langs";
import { useApp } from "../lib/store";
import type { Note } from "../lib/types";

/** Pause d'écriture avant l'enregistrement d'une note. */
const SAVE_AFTER = 600;

function when(ts: number): string {
  const d = new Date(ts * 1000);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) return new Intl.DateTimeFormat(locale(), { hour: "2-digit", minute: "2-digit" }).format(d);
  const y = new Date(today);
  y.setDate(today.getDate() - 1);
  if (d.toDateString() === y.toDateString()) return t("Hier", "Yesterday");
  return new Intl.DateTimeFormat(locale(), { day: "numeric", month: "short" }).format(d);
}

/** Le titre d'une note : le sien, sinon sa première ligne. */
function noteName(n: Pick<Note, "title" | "body">): string {
  return n.title.trim() || n.body.trim().split("\n")[0].slice(0, 80) || t("Sans titre", "Untitled");
}

/** Ce qu'on lit sous le titre dans la liste : la suite du texte. */
function noteSnippet(n: Pick<Note, "title" | "body">): string {
  const lines = n.body.trim().split("\n").filter((l) => l.trim());
  const rest = n.title.trim() ? lines : lines.slice(1);
  return rest.join(" ").slice(0, 120);
}

const words = (s: string) => (s.trim() ? s.trim().split(/\s+/).length : 0);

/**
 * Le carnet de notes d'une langue : la liste à gauche (comme les conversations du
 * chat), la note ouverte à droite, dans la police de lecture. Tout s'enregistre en
 * écrivant ; une note laissée vide disparaît en la quittant.
 */
export function Notes() {
  const lang = useApp((s) => s.lang)();
  const toast = useApp((s) => s.toast);
  const [notes, setNotes] = useState<Note[] | null>(null);
  const [openId, setOpenId] = useState<number | null>(null);
  const [draft, setDraft] = useState({ title: "", body: "" });
  const dirty = useRef(false);
  // une note qu'on vient de créer prend le curseur dès que sa page apparaît
  const [fresh, setFresh] = useState<number | null>(null);
  const body = useRef<HTMLTextAreaElement>(null);
  // iPhone : la liste, ou la note ouverte (un retour mène à la liste)
  const compact = useCompact();
  const [listOpen, setListOpen] = useState(true);
  const open = notes?.find((n) => n.id === openId) ?? null;

  useEffect(() => {
    setNotes(null);
    setOpenId(null);
    api()
      .notesList(lang)
      .then(setNotes)
      .catch((e) => toast(errorText(e), "error"));
  }, [lang, toast]);

  /**
   * Enregistre la note ouverte si elle a changé. Vide, elle n'est supprimée qu'en la
   * quittant (`leaving`) : effacer tout son texte en écrivant ne la fait pas disparaître.
   */
  const flush = useCallback(async (leaving = false) => {
    if (!open) return;
    const { title, body } = draft;
    const empty = !title.trim() && !body.trim();
    if (empty && !leaving) return;
    if (!empty && !dirty.current) return;
    dirty.current = false;
    try {
      if (empty) {
        await api().noteDelete(open.id);
        setNotes((ns) => ns?.filter((n) => n.id !== open.id) ?? ns);
        return;
      }
      const saved = await api().noteUpdate(open.id, title, body);
      // la note modifiée remonte en tête, comme une conversation
      setNotes((ns) => (ns ? [saved, ...ns.filter((n) => n.id !== saved.id)] : ns));
    } catch (e) {
      toast(errorText(e), "error");
    }
  }, [open, draft, toast]);

  // enregistrement après une courte pause d'écriture
  useEffect(() => {
    if (!dirty.current) return;
    const timer = window.setTimeout(() => void flush(), SAVE_AFTER);
    return () => window.clearTimeout(timer);
  }, [draft, flush]);

  // en quittant la vue ou la fenêtre : rien ne se perd
  const flushRef = useRef(flush);
  flushRef.current = flush;
  useEffect(() => {
    const blur = () => void flushRef.current();
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("blur", blur);
      void flushRef.current(true);
    };
  }, []);

  /** Ouvre une note, après avoir enregistré (ou retiré, si vide) celle qu'on quitte. */
  const select = async (n: Note) => {
    setListOpen(false);
    if (n.id === openId) return;
    await flush(true);
    dirty.current = false;
    setDraft({ title: n.title, body: n.body });
    setOpenId(n.id);
  };

  const create = async () => {
    try {
      const n = await api().noteCreate(lang, "", "");
      setNotes((ns) => [n, ...(ns ?? [])]);
      setFresh(n.id);
      await select(n);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  const remove = async (n: Note) => {
    const name = noteName(n.id === openId ? draft : n);
    const empty = n.id === openId ? !draft.title.trim() && !draft.body.trim() : !n.title.trim() && !n.body.trim();
    if (!empty) {
      const ok = await confirmAsk(
        t(`Supprimer « ${name} » ? Son texte sera effacé.`, `Delete “${name}”? Its text will be erased.`),
        t("Supprimer la note", "Delete the note"),
        t("Supprimer", "Delete"),
      );
      if (!ok) return;
    }
    if (n.id === openId) {
      dirty.current = false;
      setOpenId(null);
      setListOpen(true);
    }
    await api().noteDelete(n.id);
    setNotes((ns) => ns?.filter((x) => x.id !== n.id) ?? ns);
  };

  const edit = (patch: Partial<typeof draft>) => {
    dirty.current = true;
    setDraft((d) => ({ ...d, ...patch }));
  };

  const li = langInfo(lang);
  const n = words(draft.body);
  return (
    <>
      <div className="titlebar drag" data-tauri-drag-region />
      <div className={`chat-view notes-view ${compact ? (listOpen ? "m-list" : "m-thread") : ""}`}>
        <aside className="chat-side" aria-label={t("Notes", "Notes")}>
          <div className="chat-side-head">
            <h1>{t("Notes", "Notes")}</h1>
            <button className="icon-btn" onClick={() => void create()} aria-label={t("Nouvelle note", "New note")} title={t("Nouvelle note", "New note")}>
              <Icon name="edit" size={17} />
            </button>
          </div>
          <p className="chat-side-sub">{t(`Votre carnet de ${li.name.toLowerCase()}`, `Your ${li.name} notebook`)}</p>
          <div className="chat-list">
            <button className="chat-item new" onClick={() => void create()}>
              <span className="chat-item-icon">
                <Icon name="plus" size={14} stroke={2} />
              </span>
              <span className="chat-item-text">
                <strong>{t("Nouvelle note", "New note")}</strong>
              </span>
            </button>
            <AnimatePresence initial={false}>
              {(notes ?? []).map((note) => {
                const on = note.id === openId;
                const shown = on ? { ...note, ...draft } : note;
                return (
                  <motion.div key={note.id} layout="position" initial={{ opacity: 0, y: -4 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, height: 0 }} className="chat-item-row">
                    <button className={`chat-item ${on ? "on" : ""}`} onClick={() => void select(note)}>
                      {on && <motion.span layoutId="notes-pill" className="chat-pill" transition={{ type: "spring", stiffness: 500, damping: 40 }} />}
                      <span className="chat-item-icon">
                        <Icon name="note" size={14} />
                      </span>
                      <span className="chat-item-text" dir="auto">
                        <strong>{frenchSpaces(noteName(shown))}</strong>
                        <small>{noteSnippet(shown) || t("Rien d'autre pour l'instant", "Nothing else yet")}</small>
                      </span>
                      <span className="chat-item-when num">{when(note.updated_at)}</span>
                    </button>
                    <button className="chat-item-del" onClick={() => void remove(note)} aria-label={t(`Supprimer « ${noteName(shown)} »`, `Delete “${noteName(shown)}”`)} title={t("Supprimer", "Delete")}>
                      <Icon name="trash" size={14} />
                    </button>
                  </motion.div>
                );
              })}
            </AnimatePresence>
          </div>
        </aside>

        <section className="chat-main">
          <header className="chat-head">
            {compact && (
              <button className="icon-btn chat-back" onClick={() => (void flush(), setListOpen(true))} aria-label={t("Notes", "Notes")}>
                <Icon name="back" size={18} />
              </button>
            )}
            {open ? (
              <input
                className="chat-title-input notes-title"
                value={draft.title}
                dir="auto"
                placeholder={t("Titre", "Title")}
                aria-label={t("Titre de la note", "Note title")}
                onChange={(e) => edit({ title: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    body.current?.focus();
                  }
                }}
              />
            ) : (
              <h2 />
            )}
            {open && (
              <button className="icon-btn" onClick={() => void remove(open)} aria-label={t("Supprimer la note", "Delete the note")} title={t("Supprimer la note", "Delete the note")}>
                <Icon name="trash" size={16} />
              </button>
            )}
          </header>

          <AnimatePresence mode="wait" initial={false}>
            {open ? (
              <motion.div key={open.id} className="notes-page" initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ duration: 0.22, ease: [0.2, 0.8, 0.2, 1] }}>
                <textarea
                  ref={body}
                  autoFocus={fresh === open.id}
                  className="notes-body"
                  value={draft.body}
                  dir="auto"
                  lang={lang}
                  spellCheck
                  placeholder={t("Une règle de grammaire, une expression entendue, des mots à revoir…", "A grammar rule, a phrase you heard, words to review…")}
                  aria-label={t("Texte de la note", "Note text")}
                  onChange={(e) => edit({ body: e.target.value })}
                />
                <footer className="notes-foot num">
                  {count(n, "mot", "mots", "word", "words")} · {t(`modifiée ${when(open.updated_at).toLowerCase()}`, `edited ${when(open.updated_at)}`)}
                </footer>
              </motion.div>
            ) : (
              notes && (
                <motion.div key="welcome" className="chat-welcome notes-welcome" initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, transition: { duration: 0.12 } }} transition={{ duration: 0.4, ease: [0.2, 0.8, 0.2, 1] }}>
                  <div className="chat-dawn" aria-hidden="true">
                    <Orb size={compact ? 30 : 42} />
                  </div>
                  <h2>{frenchSpaces(notes.length ? t("Choisissez une note", "Pick a note") : t(`Votre carnet de ${li.name.toLowerCase()}`, `Your ${li.name} notebook`))}</h2>
                  <p>
                    {t(
                      "Ce qui mérite d'être gardé : une règle de grammaire, une expression entendue, les mots qui résistent. Chaque langue a son carnet, sauvegardé avec votre progression.",
                      "What's worth keeping: a grammar rule, a phrase you heard, the words that won't stick. Each language has its own notebook, backed up with your progress.",
                    )}
                  </p>
                  <button className="btn primary glow" onClick={() => void create()}>
                    <Icon name="edit" size={15} /> {t("Écrire une note", "Write a note")}
                  </button>
                </motion.div>
              )
            )}
          </AnimatePresence>
        </section>
      </div>
    </>
  );
}
