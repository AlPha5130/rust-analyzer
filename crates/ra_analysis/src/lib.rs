extern crate parking_lot;
#[macro_use]
extern crate log;
extern crate fst;
extern crate once_cell;
extern crate ra_editor;
extern crate ra_syntax;
extern crate rayon;
extern crate relative_path;
#[macro_use]
extern crate crossbeam_channel;
extern crate im;
extern crate rustc_hash;
extern crate salsa;

mod db;
mod descriptors;
mod imp;
mod job;
mod module_map;
mod roots;
mod symbol_index;

use std::{fmt::Debug, sync::Arc};

use ra_syntax::{AtomEdit, File, TextRange, TextUnit};
use relative_path::{RelativePath, RelativePathBuf};
use rustc_hash::FxHashMap;

use crate::imp::{AnalysisHostImpl, AnalysisImpl, FileResolverImp};

pub use crate::{
    descriptors::FnDescriptor,
    job::{JobHandle, JobToken},
};
pub use ra_editor::{
    CompletionItem, FileSymbol, Fold, FoldKind, HighlightedRange, LineIndex, Runnable,
    RunnableKind, StructureNode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Canceled;

pub type Cancelable<T> = Result<T, Canceled>;

impl std::fmt::Display for Canceled {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.write_str("Canceled")
    }
}

impl std::error::Error for Canceled {
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CrateId(pub u32);

#[derive(Debug, Clone, Default)]
pub struct CrateGraph {
    pub crate_roots: FxHashMap<CrateId, FileId>,
}

pub trait FileResolver: Debug + Send + Sync + 'static {
    fn file_stem(&self, file_id: FileId) -> String;
    fn resolve(&self, file_id: FileId, path: &RelativePath) -> Option<FileId>;
}

#[derive(Debug)]
pub struct AnalysisHost {
    imp: AnalysisHostImpl,
}

impl AnalysisHost {
    pub fn new() -> AnalysisHost {
        AnalysisHost {
            imp: AnalysisHostImpl::new(),
        }
    }
    pub fn analysis(&self) -> Analysis {
        Analysis {
            imp: self.imp.analysis(),
        }
    }
    pub fn change_file(&mut self, file_id: FileId, text: Option<String>) {
        self.change_files(::std::iter::once((file_id, text)));
    }
    pub fn change_files(&mut self, mut changes: impl Iterator<Item = (FileId, Option<String>)>) {
        self.imp.change_files(&mut changes)
    }
    pub fn set_file_resolver(&mut self, resolver: Arc<FileResolver>) {
        self.imp.set_file_resolver(FileResolverImp::new(resolver));
    }
    pub fn set_crate_graph(&mut self, graph: CrateGraph) {
        self.imp.set_crate_graph(graph)
    }
    pub fn add_library(&mut self, data: LibraryData) {
        self.imp.add_library(data.root)
    }
}

#[derive(Debug)]
pub struct SourceChange {
    pub label: String,
    pub source_file_edits: Vec<SourceFileEdit>,
    pub file_system_edits: Vec<FileSystemEdit>,
    pub cursor_position: Option<Position>,
}

#[derive(Debug)]
pub struct Position {
    pub file_id: FileId,
    pub offset: TextUnit,
}

#[derive(Debug)]
pub struct SourceFileEdit {
    pub file_id: FileId,
    pub edits: Vec<AtomEdit>,
}

#[derive(Debug)]
pub enum FileSystemEdit {
    CreateFile {
        anchor: FileId,
        path: RelativePathBuf,
    },
    MoveFile {
        file: FileId,
        path: RelativePathBuf,
    },
}

#[derive(Debug)]
pub struct Diagnostic {
    pub message: String,
    pub range: TextRange,
    pub fix: Option<SourceChange>,
}

#[derive(Debug)]
pub struct Query {
    query: String,
    lowercased: String,
    only_types: bool,
    libs: bool,
    exact: bool,
    limit: usize,
}

impl Query {
    pub fn new(query: String) -> Query {
        let lowercased = query.to_lowercase();
        Query {
            query,
            lowercased,
            only_types: false,
            libs: false,
            exact: false,
            limit: usize::max_value(),
        }
    }
    pub fn only_types(&mut self) {
        self.only_types = true;
    }
    pub fn libs(&mut self) {
        self.libs = true;
    }
    pub fn exact(&mut self) {
        self.exact = true;
    }
    pub fn limit(&mut self, limit: usize) {
        self.limit = limit
    }
}

#[derive(Debug)]
pub struct Analysis {
    imp: AnalysisImpl,
}

impl Analysis {
    pub fn file_syntax(&self, file_id: FileId) -> File {
        self.imp.file_syntax(file_id).clone()
    }
    pub fn file_line_index(&self, file_id: FileId) -> Arc<LineIndex> {
        self.imp.file_line_index(file_id)
    }
    pub fn extend_selection(&self, file: &File, range: TextRange) -> TextRange {
        ra_editor::extend_selection(file, range).unwrap_or(range)
    }
    pub fn matching_brace(&self, file: &File, offset: TextUnit) -> Option<TextUnit> {
        ra_editor::matching_brace(file, offset)
    }
    pub fn syntax_tree(&self, file_id: FileId) -> String {
        let file = self.imp.file_syntax(file_id);
        ra_editor::syntax_tree(&file)
    }
    pub fn join_lines(&self, file_id: FileId, range: TextRange) -> SourceChange {
        let file = self.imp.file_syntax(file_id);
        SourceChange::from_local_edit(file_id, "join lines", ra_editor::join_lines(&file, range))
    }
    pub fn on_enter(&self, file_id: FileId, offset: TextUnit) -> Option<SourceChange> {
        let file = self.imp.file_syntax(file_id);
        let edit = ra_editor::on_enter(&file, offset)?;
        let res = SourceChange::from_local_edit(file_id, "on enter", edit);
        Some(res)
    }
    pub fn on_eq_typed(&self, file_id: FileId, offset: TextUnit) -> Option<SourceChange> {
        let file = self.imp.file_syntax(file_id);
        Some(SourceChange::from_local_edit(
            file_id,
            "add semicolon",
            ra_editor::on_eq_typed(&file, offset)?,
        ))
    }
    pub fn file_structure(&self, file_id: FileId) -> Vec<StructureNode> {
        let file = self.imp.file_syntax(file_id);
        ra_editor::file_structure(&file)
    }
    pub fn folding_ranges(&self, file_id: FileId) -> Vec<Fold> {
        let file = self.imp.file_syntax(file_id);
        ra_editor::folding_ranges(&file)
    }
    pub fn symbol_search(&self, query: Query) -> Cancelable<Vec<(FileId, FileSymbol)>> {
        self.imp.world_symbols(query)
    }
    pub fn approximately_resolve_symbol(
        &self,
        file_id: FileId,
        offset: TextUnit
    ) -> Cancelable<Vec<(FileId, FileSymbol)>> {
        self.imp
            .approximately_resolve_symbol(file_id, offset)
    }
    pub fn find_all_refs(&self, file_id: FileId, offset: TextUnit, ) -> Cancelable<Vec<(FileId, TextRange)>> {
        Ok(self.imp.find_all_refs(file_id, offset))
    }
    pub fn parent_module(&self, file_id: FileId) -> Cancelable<Vec<(FileId, FileSymbol)>> {
        self.imp.parent_module(file_id)
    }
    pub fn crate_for(&self, file_id: FileId) -> Cancelable<Vec<CrateId>> {
        self.imp.crate_for(file_id)
    }
    pub fn crate_root(&self, crate_id: CrateId) -> Cancelable<FileId> {
        Ok(self.imp.crate_root(crate_id))
    }
    pub fn runnables(&self, file_id: FileId) -> Cancelable<Vec<Runnable>> {
        let file = self.imp.file_syntax(file_id);
        Ok(ra_editor::runnables(&file))
    }
    pub fn highlight(&self, file_id: FileId) -> Cancelable<Vec<HighlightedRange>> {
        let file = self.imp.file_syntax(file_id);
        Ok(ra_editor::highlight(&file))
    }
    pub fn completions(&self, file_id: FileId, offset: TextUnit) -> Cancelable<Option<Vec<CompletionItem>>> {
        let file = self.imp.file_syntax(file_id);
        Ok(ra_editor::scope_completion(&file, offset))
    }
    pub fn assists(&self, file_id: FileId, range: TextRange) -> Cancelable<Vec<SourceChange>> {
        Ok(self.imp.assists(file_id, range))
    }
    pub fn diagnostics(&self, file_id: FileId) -> Cancelable<Vec<Diagnostic>> {
        self.imp.diagnostics(file_id)
    }
    pub fn resolve_callable(
        &self,
        file_id: FileId,
        offset: TextUnit,
    ) -> Cancelable<Option<(FnDescriptor, Option<usize>)>> {
        self.imp.resolve_callable(file_id, offset)
    }
}

#[derive(Debug)]
pub struct LibraryData {
    root: roots::ReadonlySourceRoot,
}

impl LibraryData {
    pub fn prepare(files: Vec<(FileId, String)>, file_resolver: Arc<FileResolver>) -> LibraryData {
        let file_resolver = FileResolverImp::new(file_resolver);
        let root = roots::ReadonlySourceRoot::new(files, file_resolver);
        LibraryData { root }
    }
}

#[test]
fn analysis_is_send() {
    fn is_send<T: Send>() {}
    is_send::<Analysis>();
}
