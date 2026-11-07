app-title = Attic
app-comment = Retro-flavoured markdown notes
app-keywords = notes;markdown;retro;COSMIC
about = About
repository = Repository
view = View
file = File
git-description = Git commit {$hash} on {$date}

# Navigation
nav-all-notes = Notes
nav-untagged = Untagged
nav-trash = Trash
nav-tags = Tags

# Actions
new-note = New note
trash-note = Move to trash
restore-note = Restore
delete-forever = Delete forever
empty-trash = Empty trash
pin-note = Pin
unpin-note = Unpin
search-placeholder = Search notes

# Editor
untitled = Untitled
no-note-selected = Select a note, or press Ctrl+N to start one
no-notes-here = Nothing here yet
no-results = No notes match "{ $query }"
linked-from = Linked from
trash-hint = This note is in the trash. Restore it to edit.
notes-count = { $count ->
    [one] 1 note
   *[other] { $count } notes
}
store-error = Couldn't open the notes folder: { $error }
import-error = Couldn't add { $file }: { $error }

# Retro frames
frame-views = views
frame-tags = tags
tag-rename = rename…
tag-icon = icon…
tag-icon-none = none
tag-rename-placeholder = new name
frame-notes = notes
no-tags-yet = no tags yet
notes-of = { $shown } of { $total }
badge-in-trash = in trash
badge-saved = ✓ saved { $time }
badge-not-saved = ✗ not saved — last saved { $time }

# Dock
dock-bold = Bold
dock-italic = Italic
dock-mark = Highlight
dock-table = Table
table-add-row = + row
table-add-col = + column
table-del-row = − row
table-del-col = − column
table-money = $ ¤
table-fx = fx
table-add-row-hint = Add a row below
table-add-col-hint = Add a column at the right
table-del-row-hint = Remove this row
table-del-col-hint = Remove this column
table-money-hint = Money format: cycles $ € £ ¥ and back to plain. Sums of money come out as money.
table-hint = Click a cell to edit; = starts a formula (SUM, AVG, MIN, MAX…). Type $ for money. Drag the edges to resize, and drag the little corner dot to fill a formula down or across.
section-tables = Tables
table-toolbar-toggle = Show the table toolbar
table-toolbar-hint = The +/− row and column and money buttons above a table while a cell is open. Keyboard and formulas work either way.
dock-code = Code
dock-h1 = Heading 1
dock-h2 = Heading 2
dock-bullet = Bullet list
dock-todo = To-do
dock-link = Wiki link
dock-tag = Tag
dock-rule = Divider
dock-quote = Quote
dock-plus = New note or folder
dock-new-note = note
dock-new-folder = folder
folder-name-placeholder = folder name

# Themes
theme-colours = Settings
appearance = Options
section-colour = Colour
font-pairings-hint = Designer pairings set the pane titles, the sidebar and list, and the note together.
font-restore = Restore default fonts
quit = Quit
font-target = Apply to
target-tags = tags
target-note = notes
target-editor = editor
font-size-label = size
font-base = Base editor font
font-weight = Weight
font-weight-hint = For the note body. Faces missing a weight use the closest they have.
font-more = More fonts
font-chefs = Chefs Selection
pairing-sample-ui = views · tags · notes · 12:50
pairing-sample-body = The quick brown fox jumps over the lazy dog.
section-font = Font
section-size = Size
dock-size = Dock size
size-sidebar = Sidebar
size-list = Notes list
size-editor = Editor
size-sample-sidebar = # travels / japan · 5
size-sample-list = Kyoto convenience store — walking past the shop on a cold night
size-sample-editor = The quick brown fox jumps over the lazy dog.
size-rule = Divider line
size-rule-hint = How thick a --- line draws. Family size feeds the whole page.
size-tag-line = Sidebar spacers
size-tag-line-hint = The horizontal lines between tag groups in the sidebar.
size-card = File and link cards
card-border-toggle = Draw a border around cards
text-width = Text width
text-width-hint = How wide the note gets before it centres with margins. A narrower window always wraps to fit.
section-tasks = Tasks
section-icon = Icon
icon-set = Folder icons
icon-set-hint = The style of the icons folders wear. Boxicons has every icon; the others borrow it for any they lack.
coffee-caption = COFFEE
coffee-hint = open 24h · any key to close
icon-follow = Follow the colour theme
icon-hint = The launcher icon: two jots and a hash, in the colours of any theme. It is written into your icon theme, so the dock and app library update on their own.
task-marker-hint = What a finished task shows inside its brackets. It is written into the file, so other markdown apps only recognise the x as "done".
task-sample-open = - [ ] pick up milk
task-sample-done = - [{ $mark }] walk the duck
show-markers = Show markdown syntax
theme-picker-hint = Retro palettes, a plain white, five from COLOURlovers, and one that follows your desktop. Click to apply.
theme-minimal-header = Minimal
theme-minimal-hint = One colour on charcoal: highlights, tags, the caret, H1 and H2. Everything else stays quiet.
section-buffet = Colour buffet
buffet-hint = Serve yourself: a highlight, then a plate from either side. A click applies it; picking a theme above switches the buffet off.
buffet-highlight = Highlight
buffet-dark-mode = Dark mode
buffet-light-mode = Light mode
buffet-paper = Paper
buffet-ink = Ink
buffet-serving-dark = Serving { $hi } on { $plate }.
buffet-serving-light = Serving { $hi } on { $paper }, writing in { $ink }.
buffet-off = Nothing plated yet — click a colour to switch the buffet on.

# Layout
show-nav = Views and tags
show-list = Notes list
editor-only = Editor only
zoom-in = Bigger text
zoom-out = Smaller text
zoom-reset = Reset text size

# Images
images-rail = Images: right rail
images-top = Images: top strip
images-bottom = Images: bottom strip
dock-image = Add image
pick-image = Add an image to this note
image-files = Images
open-image = open

# Picker
picker-home = Home
picker-pictures = Pictures
picker-downloads = Downloads
picker-empty = No folders or images here.
picker-empty-folders = No folders here.
picker-grid = Grid
picker-list = List
badge-drop = drop to add

# Image menu
menu-frame = frame
menu-align = sits
menu-width = width
menu-full = full
menu-resize-hint = drag ◢ on the picture to resize
menu-move-hint = drag the picture itself to move it to another line
drop-here = ▼ drops here
menu-remove = remove
menu-caption = caption / title
caption-placeholder = name this picture

# Edit menu
edit = Edit
undo = Undo
redo = Redo

# Shortcuts
format = Format
new-folder = New folder
search-notes = Search notes
shortcuts = Keyboard shortcuts
shortcuts-hint = Every shortcut in one place. Esc or × to close.

# Link cards
section-links = Links
link-previews = Fetch link previews
link-previews-hint = A web address on a line of its own becomes a card with the page's title, description and picture. Fetching one tells that site you saved its link; switch it off to keep cards to the address alone.
dock-attach = Attach file
attach-hint = Any file dropped on the note (or picked here) is copied into assets and shown as a card. Double-click a card to open it; drag it to move it.
link-open = open
link-copy = copy link
link-refresh = refresh preview
link-fetching = fetching…
link-offline = no preview
file-missing = file not found

animation = Animation
animation-hint = The drag drop line and friends. Time is the whole trip; landing is how hard it brakes at the end — 1.0 barely slows, higher floats in.
anim-time = time
anim-landing = landing
anim-off = off

# Options → Notes folder
section-folder = Notes folder
folder-hint = Every note, picture and attachment lives here as a plain file. Put this folder in Dropbox (or any synced folder) and choose the same folder on your other machines: changes show up here as they arrive.
folder-choose = Choose folder…
folder-default = Use the default
folder-use-this = Use this folder
pick-folder = Choose the notes folder
picker-documents = Documents
picker-dropbox = Dropbox
other-device = another device

# Options → Sync
section-sync = Sync
sync-hint = Keep these notes on every machine through your own server. Notes, pictures and attached files travel as sealed packages the server never reads.
sync-url = Server
sync-url-placeholder = https://notes.example.com
sync-email = Email
sync-email-placeholder = you@example.com
sync-password = Password
sync-password-placeholder = at least 8 characters
sync-sign-in = Sign in
sync-create-account = Create account
sync-sign-out = Sign out
sync-now = Sync now
sync-signing-in = Signing in…
sync-signed-in = Signed in as { $email } on { $url }
sync-signed-out = Signed out — sign in again to keep syncing.
sync-syncing = Syncing…
sync-waiting = Waiting for the first sync.
sync-last = Synced { $ago }.
sync-error = Could not sync: { $error }
sync-pending = { $n ->
    [one] One note waiting to go up.
   *[other] { $n } notes waiting to go up.
}
sync-moved = { $down ->
    [0] Nothing new from other devices
    [one] One note came down
   *[other] { $down } notes came down
}, { $up ->
    [0] nothing sent up.
    [one] one sent up.
   *[other] { $up } sent up.
}
sync-files-pending = { $n ->
    [one] One picture or file waiting to go up.
   *[other] { $n } pictures and files waiting to go up.
}
sync-files-moved = Pictures and files: { $down ->
    [0] none came down
    [one] one came down
   *[other] { $down } came down
}, { $up ->
    [0] none sent up.
    [one] one sent up.
   *[other] { $up } sent up.
}
sync-conflicts = { $n ->
    [one] One note changed here and elsewhere: your version is kept beside it as "(conflict, { $device })".
   *[other] { $n } notes changed here and elsewhere: your versions are kept beside them as "(conflict, { $device })".
}
sync-server-hint = Any PocketBase server with the Attic collection will do — see server/README.md in the source for a five-minute setup.
ago-now = just now
ago-seconds = { $n } s ago
ago-minutes = { $n ->
    [one] a minute ago
   *[other] { $n } min ago
}
ago-hours = { $n ->
    [one] an hour ago
   *[other] { $n } h ago
}
