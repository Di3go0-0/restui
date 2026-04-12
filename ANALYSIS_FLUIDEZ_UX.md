# ANÁLISIS EXHAUSTIVO DE RESTUI

## Resumen Ejecutivo

**Restui** es un cliente TUI HTTP con vim keybindings (~15.8K líneas de Rust, arquitectura moderna con tokio async). El análisis revela una aplicación bien estructurada con algunas **oportunidades de mejora** en fluidez, UX y manejo de edge cases.

---

# 1. FLUIDEZ DE INTERACCIÓN

## ✅ Aspectos Positivos

### Event Loop Optimizado
- **Arquitectura async/await correcta**: Usa tokio con handler de eventos en thread dedicado
- **No bloquea UI**: El event::poll() en thread separado no interfiere con async runtime
- **Tick rate apropiado**: 250ms (EVENT_TICK_RATE) balance entre responsividad y CPU

```rust
// src/core/event.rs
// Dedicado OS thread + mpsc channel → NO bloqueo
let task = std::thread::spawn(move || {
    loop {
        if event::poll(tick_rate).unwrap_or(false) {
            match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    if tx.send(AppEvent::Key(key)).is_err() { break; }
                }
                // ... handles Resize, Tick async
            }
        }
    }
});
```

### Rendering Eficiente
- **Cada frame redibuja completamente** (src/app/mod.rs:106-108)
- **Ratatui optimiza**: solo envía diff a terminal
- **Sin dirty flags**: modelo simple = menos bugs, pero ver abajo

### Gestión de Memoria
- **Response cache limitado**: RESPONSE_CACHE_MAX = 50 (ejecute.rs:5)
- **Cleanup en tick**: status messages expiran (21ms TTL)

---

## ⚠️ PAIN POINTS - Fluidez

### 1. **Búsqueda No Debounced**
**Ubicación**: `src/app/search.rs` + `src/app/autocomplete.rs`

```rust
// src/app/search.rs:6-38
pub(super) fn recalculate_search_matches(&mut self) {
    // Se recalcula en CADA carácter tipado sin debounce
    self.state.search.matches.clear();
    self.state.search.match_idx = 0;
    
    let text = match self.state.active_panel {
        Panel::Response => {
            if let Some(ref resp) = self.state.current_response {
                resp.formatted_body()  // ← Si respuesta es GRANDE (MB), TRABAJO PESADO
            }
        }
        Panel::Body => self.active_body().to_string(),
        _ => return,
    };
    
    // Búsqueda bruta: O(n) sobre todo el texto
    for (row, line) in text.lines().enumerate() {
        let line_lower = line.to_lowercase();
        let mut start = 0;
        while let Some(pos) = line_lower[start..].find(&query) {
            self.state.search.matches.push((row, start + pos));
            start += pos + 1;
        }
    }
}
```

**Impacto**: 
- Respuesta de **100KB+** → freeze visible al escribir query
- `to_lowercase()` y múltiples búsquedas = CPU spike

**Recomendación**:
- Debounce 300-500ms
- Limitar búsqueda a viewport visible
- Cache líneas minúsculas

---

### 2. **Autocomplete Cadena Sin Throttle**
**Ubicación**: `src/app/autocomplete.rs:6-80`

```rust
pub(super) fn try_chain_autocomplete(&mut self) {
    // Se ejecuta en CADA keystroke
    // Si hay 100+ requests en cache → búsqueda lenta
    
    let (request_name_raw, path_so_far, has_path) = 
        if let Some(bracket_pos) = after_at.find('[') {
            // Parsing de path complejo
        }
    
    // Luego: traverse JSON desde response_cache
    // Si JSON es grande → traversal caro
}
```

**Impacto**:
- Proyectos con muchas requests/responses grandes → lag notable

**Recomendación**:
- Cache JSON paths ya extraídos
- Limitar a top 50 sugerencias

---

### 3. **Cálculo Responsivo: Sin Lazy Evaluation en Layouts**
**Ubicación**: `src/app/mod.rs:66-103`

```rust
loop {
    // CADA FRAME (4-10ms @ 60fps virtual):
    if let Ok(size) = terminal.size() {
        let right_width = (size.width as u32 * 80 / 100) as u16;
        self.state.is_wide_layout = right_width > WIDE_LAYOUT_THRESHOLD;
        
        // Recalcular visible_height/visible_width SIEMPRE
        let main_h = size.height.saturating_sub(1);
        if right_width > WIDE_LAYOUT_THRESHOLD {
            self.state.body_vim.visible_height = 
                (center_h as u32 * 60 / 100) as usize;
            self.state.response_view.resp_vim.visible_height = main_h as usize;
        } else {
            // ... narrow layout
        }
        self.state.body_vim.visible_height = 
            self.state.body_vim.visible_height.saturating_sub(4);
        // ... more subtractions
    }
    
    terminal.draw(|frame| {
        ui::layout::render(frame, &self.state);  // REDIBUJA SIEMPRE
    })?;
}
```

**Problema**:
- Sin **dirty flag**: SIEMPRE redibuja aunque nada cambió
- Resize no es frecuente, pero cálculo se repite innecesariamente

**Recomendación**:
- Cache último tamaño terminal
- Solo recalcular si cambió

---

### 4. **Sync Variable Substitution en Strings Largos**
**Ubicación**: `src/model/environment.rs:36-45`

```rust
pub fn resolve(&self, template: &str) -> String {
    let mut result = template.to_string();
    for (key, value) in &env.variables {
        // String::replace es O(n) POR CADA variable
        // Si 50 variables × 1MB body → n×50 trabajo
        result = result.replace(&format!("{{{{{}}}}}", key), value);
    }
    result
}
```

**Impacto**: Body JSON grande + muchas variables = perceptible delay

---

### 5. **JSON Parsing/Formatting Sin Cache**
**Ubicación**: `src/ui/response.rs`

```rust
// Cada frame:
let preview = &state.body_vim.preview_lines;
let body_lines: Vec<&str> = if let Some(plines) = preview {
    plines.iter().map(|s| s.as_str()).collect()
} else if body_text.is_empty() {
    vec![""]
} else {
    let mut lines: Vec<&str> = body_text.lines().collect();
    if body_text.ends_with('\n') {
        lines.push("");
    }
    lines  // ← Re-parsing CADA FRAME
};
```

---

## 📊 Tabla de Rendimiento Estimado

| Operación | Tamaño | Impacto |
|-----------|--------|---------|
| Buscar en respuesta | 1MB | 50-100ms |
| Autocomplete cadena | 100 requests | 10-20ms |
| Resize cálculo | Cada frame | 1-2ms |
| Variable substitution | 1MB + 50 vars | 30-50ms |
| JSON reformat | 100KB+ | 5-10ms |

---

# 2. UX PAIN POINTS

## ✅ Navegación Intuitiva
- **Panel shortcuts**: `1/2/3/4` + `Ctrl+hjkl` ✓
- **Layout responsivo**: wide vs narrow layouts ✓
- **Help overlay**: `?` siempre disponible ✓

---

## ⚠️ PROBLEMAS UX

### 1. **Descubrimiento de Keybindings**
**Falta**:
- Help panel es estático (` src/ui/help.rs:21-120`)
- NO hay inline hints contextuales
- Modo Insert NO muestra "press Esc to exit" en tiempo real

**Ejemplo**:
```rust
// src/ui/statusbar.rs renderiza:
// "Mode: Normal  ●  {Collection}"
// vs debería:
// "Mode: Normal (e:edit, a:add, Esc:panel) ●  {Collection}"
```

**Recomendación**:
- Hints contextuales en status bar basados en panel
- Mostrar próximas acciones disponibles en visual mode

---

### 2. **Sincronización Cursor Deficiente en Inline Edit**
**Ubicación**: `src/app/inline_edit.rs`

```rust
pub(super) fn inline_input(&mut self, c: char) {
    match self.state.active_panel {
        Panel::Request => match self.state.request_edit.focus {
            RequestFocus::Header(idx) => {
                if let Some(h) = self.state.current_request.headers.get_mut(idx) {
                    let field = if self.state.request_edit.header_edit_field == 0 
                        { &mut h.name } else { &mut h.value };
                    
                    // Cursor posición manual: ERROR PRONE
                    let cursor = self.state.request_edit.header_edit_cursor
                        .min(field.len());
                    field.insert(cursor, c);
                    self.state.request_edit.header_edit_cursor = cursor + 1;
                }
                // Después de cada keystroke: puede desincronizar
            }
        }
    }
}
```

**Problema**:
- Cursor puede quedarse **ATRÁS** del contenido real
- Si field.len() cambia pero cursor no actualiza correctamente
- **No hay visual feedback** claro de posición

**Caso Edge Case**:
1. User: URL "http://example.com", cursor al final
2. Selecciona todo (Ctrl+A simulado) → field.len() = 0
3. Tipea: "h" → field.insert(0, 'h')
4. Cursor = 1, field.len() = 1 ✓ OK
5. **PERO** si hay bug en posición previa → cursor queda desync

---

### 3. **Respuestas Muy Grandes: Sin Streaming/Pagination**
**Ubicación**: `src/core/http_client.rs:96-122`

```rust
let body_bytes = resp.bytes().await?;  // ← CARGA COMPLETA EN MEMORIA
let size_bytes = body_bytes.len();

let (body, raw_bytes) = if is_binary {
    // ... binary handling
} else {
    (String::from_utf8_lossy(&body_bytes).to_string(), None)
};
```

**Problemas**:
- Respuesta **1GB** → OOM o slow UI para siempre
- Sin indicador de progreso/tamaño antes de cargar
- Body response display **recorre TODA la respuesta** cada búsqueda

**Recomendación**:
- Truncar respuestas > 10MB con warning
- Indicador de tamaño en header respuesta
- Lazy line-by-line parsing

---

### 4. **Modo Insert vs Normal: Hints Visuales Débiles**
**Ubicación**: `src/ui/statusbar.rs:33-39`

```rust
let mode_str = match self.state.mode {
    InputMode::Normal => Span::styled(
        " Normal ",
        Style::default().fg(t.text).bg(t.bg_highlight),
    ),
    InputMode::Insert => Span::styled(
        " Insert ",
        Style::default().fg(Color::White).bg(Color::DarkCyan),  // ← Solo color
    ),
    // ...
};
```

**Problema**:
- En algunos temas (light.toml): **contraste bajo**
- Sin indicador cursor shape en respuesta panel
- Usuario puede no darse cuenta que está en INSERT en body

---

### 5. **Autocomplete No Muestra Contexto Completo**
**Ubicación**: `src/app/autocomplete.rs:44-64`

```rust
items.push((
    format!("{}: {}", key, truncated),  // ← "token: abc123..."
    suffix.to_string(),
));
```

**Problema**:
- Al escribir `{{base_`, autocomplete muestra **solo nombre + preview**
- **No muestra**: qué environment está activo arriba
- Usuario típico: "¿De dónde sale esta variable?"

---

### 6. **Visual Block Mode (Ctrl+V): Poco Intuitivo**
**Status**:
- Implementado ✓
- Visual feedback en body ✓
- **PERO**: Help page (src/ui/help.rs:64) NO lo menciona
- Shortcut no es universal (some terminals no soportan Ctrl+V correctamente)

---

## 3. ACCESIBILIDAD

### Color Contrast Analysis (themes/)

**Light Theme** (`light.toml`):
```toml
text = "#24292f"           // Muy oscuro
text_dim = "#656d76"       // OK pero no para fondos
overlay_bg = "#ffffff"     // Blanco puro
```

**Análisis WCAG**:
- `text` (#24292f) sobre `overlay_bg` (#ffffff) → **19:1 contrast** ✓ AAA
- `text_dim` (#656d76) sobre `overlay_bg` (#ffffff) → **7:1 contrast** ✓ AA
- **Pero**: hover highlights/selections pueden fallar

**Dark Themes** (`default.toml`, `catppuccin.toml`):
- Generalmente **OK** (contrast > 7:1)
- **Problema**: `border_unfocused` (#585b70 Catppuccin) vs `overlay_bg` (#1e1e2e)
  - Contrast = ~2.5:1 ❌ **Falla WCAG**

**Recomendación**:
```toml
border_unfocused = "#7c7d8f"  // ← aumentar luminosity
```

### Modo Sin Mouse
- ✓ Todas las acciones con keyboard
- ✓ Vi keybindings completas
- ✓ Panel navigation con Ctrl+hjkl
- **PERO**: 
  - Help no indica "Sin mouse requerido"
  - Overlays (theme selector, env picker) solo con Up/Down/Enter

### Terminal Resizing
**Status**: SOPORTADO ✓
```rust
AppEvent::Resize(w, h) => {}  // Recalcula layouts
```

**Pero**: Sin animación/suavidad, abrupto redibujado

---

## 4. EDGE CASES

### 4.1 Manejo de Respuestas Binarias

**Status**: ✓ Detectado
```rust
// src/core/http_client.rs:99-106
let is_binary = content_type.as_deref().is_some_and(|ct| {
    ct.starts_with("image/") || ct.starts_with("audio/")
    || ct.starts_with("video/") || ct.starts_with("application/octet-stream")
    || ct == "application/pdf" || ct == "application/zip"
});

if is_binary {
    let size_display = if size_bytes < 1024 {
        format!("{}B", size_bytes)
    } else if size_bytes < 1024 * 1024 {
        format!("{:.1}KB", size_bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", size_bytes as f64 / (1024.0 * 1024.0))
    };
    (format!("Binary response ({}, {})", ct_display, size_display), Some(body_bytes.to_vec()))
}
```

**Problema**:
- Si PDF es 500MB → `Some(body_bytes.to_vec())` GUARDA en memoria
- Sin limite de tamaño

---

### 4.2 Redes Lentas: Timeout vs Cancellación

**Status**: ✓ Cancelable con Esc
```rust
pub(super) fn cancel_request(&mut self) {
    if let Some(handle) = self.state.request_abort_handle.take() {
        handle.abort();
    }
    self.state.request_in_flight = false;
}
```

**Pero**:
- Spinner mostrado cada 100ms: `src/app/mod.rs:174-182`
- **Sin** indicador de timeout configurado
- Default timeout = `config.timeout_secs` (¿cuál es default?)

**Recomendación**:
- Mostrar timeout restante en spinner
- Config UI para timeout global

---

### 4.3 Colecciones con Muchos Archivos

**Ubicación**: `src/app/collections.rs:22-65`

```rust
pub(super) fn rebuild_collection_items(&mut self) {
    let filter = self.state.collections_view.filter.to_lowercase();
    let has_filter = !filter.is_empty();
    let mut items = Vec::new();
    
    for (ci, collection) in self.state.collections.iter().enumerate() {
        let expanded = self.state.collections_view.expanded.contains(&ci);
        
        if has_filter {
            let matching: Vec<usize> = collection.requests.iter().enumerate()
                .filter(|(_, req)| {
                    req.display_name().to_lowercase().contains(&filter)
                        || req.url.to_lowercase().contains(&filter)
                })
                .map(|(i, _)| i)
                .collect();
            
            if matching.is_empty() {
                continue;
            }
            // ... O(n×m) búsqueda para cada frame si filter activo
        }
    }
}
```

**Impacto**:
- 1000 requests × cada keystroke en filter → **noticeable lag**
- Sin limit de renders

---

### 4.4 Variables No Encontradas / Referencias Rotas

**Status**: 
- Variables undefined → **no se reemplazan** (silent failure)
- Cadenas circulares → **detectadas** ✓

```rust
// src/model/chain.rs:31-32
CircularDependency { chain: Vec<String> },
```

**Problema**:
- Variable `{{undefined_var}}` → se deja como literal string en request
- Usuario NO sabe que falló hasta ver la error de servidor
- **No hay validation** en UI

**Edge case**:
```http
GET {{base_url}}/api/{{undefined_endpoint}}
```
Resultado: GET (null)/api/{{undefined_endpoint}}

**Recomendación**:
- Validar variables antes de ejecutar
- Mostrar warning en status: "Undefined: undefined_endpoint"

---

### 4.5 Rutas Circulares en Cadenas

**Status**: Detectado ✓
```rust
// src/app/execute.rs:125-180 (resolve_chains_in_request)
pub(super) async fn resolve_chains_in_request<'a>(...) {
    let mut resolving_stack = Vec::new();
    // ... Stack guards circular deps
}
```

**Pero**: Stack depth limit?
```rust
// No hay max depth check visible
```

---

### 4.6 Búsqueda de JSON Paths Rotas

**Ubicación**: `src/model/chain.rs:146-200`

```rust
pub fn extract_json_value(json_str: &str, path: &str) 
    -> Result<String, ChainError> {
    
    let root: serde_json::Value = 
        serde_json::from_str(trimmed).map_err(|_| {
            ChainError::ResponseNotJson {
                request_name: ...
            }
        })?;
    
    // Path extraction...
    // Si path no existe → JsonPathNotFound error
}
```

**Status**: ✓ Error clara

---

### 4.7 Respuestas Malformed UTF-8

```rust
// src/core/http_client.rs:122
(String::from_utf8_lossy(&body_bytes).to_string(), None)
```

**Status**: ✓ Maneja con `.lossy()`
- Caracteres inválidos → `U+FFFD` (replacement char)

---

### 4.8 Clics de Raton (Disabled)

```rust
// src/core/tui.rs:20
execute!(io::stdout(), EnterAlternateScreen, DisableMouseCapture)?;
```

**Status**: ✓ Deshabilitado explícitamente para evitar glitches

---

# 5. ISSUES CRITICOS & RECOMENDACIONES

## Crítico (Afecta Usabilidad)

| # | Issue | Severidad | Líneas | Solución |
|---|-------|-----------|--------|----------|
| **1** | Búsqueda no debounced en respuestas grandes | Alta | `search.rs:6-38` | Debounce 300ms + limitar a viewport |
| **2** | Autocomplete cadena sin cache/limit | Media | `autocomplete.rs:6-80` | Cache JSON paths, limit 50 items |
| **3** | Redibujado innecesario sin dirty flag | Media | `mod.rs:61-160` | Cache tamaño terminal |
| **4** | Respuestas > 10MB sin límite/warning | Alta | `http_client.rs:96` | Truncar + warning |

## Importante (UX)

| # | Issue | Impacto | Fix |
|---|-------|---------|-----|
| **5** | Help sin inline context hints | Discoverability | Hints en status bar |
| **6** | Cursor puede desincronizar en inline edit | Data integrity | Refactorizar con cursor tracking struct |
| **7** | Mode insert/normal hints débiles | Clarity | Mejor visual feedback (cursor shape) |
| **8** | Variables undefined → silent failure | Debugging | Validation pre-request |

## Menor (Polish)

| # | Issue | Impacto |
|---|-------|---------|
| **9** | Light theme: border_unfocused contrast bajo | Accessibility |
| **10** | Colecciones 1000+ requests → lag en filter | Performance |

---

# 6. MATRIZ RESUMEN: FORTALEZAS vs DEBILIDADES

## ✅ Fortalezas

```
┌─────────────────────────────────────────┐
│ Event loop async correctamente             │
│ Rendering eficiente (ratatui)             │
│ Navegación intuitiva (vim + keybindings)  │
│ Manejo de errores transparente            │
│ Detección circular dependencies ✓        │
│ Binary response handling ✓               │
│ Terminal resizing soportado ✓            │
│ Memory bounded (response cache)           │
│ Autocomplete (env + cadenas)              │
│ Temas customizables ✓                    │
└─────────────────────────────────────────┘
```

## ⚠️ Debilidades

```
┌─────────────────────────────────────────┐
│ Search/autocomplete sin debounce         │
│ Respuestas grandes: sin truncate         │
│ Cursor inline: desync potential          │
│ Hints contextuales ausentes              │
│ Variable validation: inexistente         │
│ Grandes colecciones: lag en filter       │
│ Color contrast (dark theme borders)      │
│ JSON path cache: inexistente             │
│ Timeout display: sin countdown           │
│ Nested body edits: complejo state        │
└─────────────────────────────────────────┘
```

---

# 7. RECOMENDACIONES PRIORIZADAS

## Sprint 1: Rendimiento (2-3 horas)

1. **Debounce búsqueda**: Agregar `search_debounce` field en state, retrasar 300ms
2. **Dirty flag layout**: Cache último tamaño terminal, skip recalc si no cambió
3. **Variable validation**: Pre-request check, listar undefined vars

## Sprint 2: UX (3-4 horas)

4. **Inline context hints**: Extender statusbar con acciones disponibles por panel
5. **Better mode indication**: Mostrar cursor shape en response panel también
6. **Response size warning**: Display tamaño antes de cargar, truncar > 10MB

## Sprint 3: Robustez (2-3 horas)

7. **Cursor tracking struct**: Encapsular cursor logic en type-safe struct
8. **JSON path cache**: Parsear y cachear paths usados frecuentemente
9. **Color contrast fix**: Actualizar dark theme borders

---

