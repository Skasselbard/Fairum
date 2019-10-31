use crate::petri_net::function::Function;
use pnml::{NodeRef, PNMLDocument, PageRef, PetriNetRef, Result};
use rustc::hir::def_id::DefId;
use rustc::mir::visit::Visitor;
use rustc::mir::visit::*;
use rustc::mir::{self, *};
use rustc::ty::{self, ClosureSubsts, GeneratorSubsts, Ty, TyCtxt};

struct CallStack<T> {
    stack: Vec<T>,
}

impl<T> CallStack<T> {
    pub fn new() -> Self {
        CallStack { stack: Vec::new() }
    }

    pub fn push(&mut self, item: T) {
        self.stack.push(item)
    }

    pub fn pop(&mut self) -> Option<T> {
        self.stack.pop()
    }

    pub fn peek(&self) -> Option<&T> {
        if self.stack.is_empty() {
            None
        } else {
            Some(&self.stack[self.stack.len() - 1])
        }
    }

    pub fn peek_mut(&mut self) -> Option<&mut T> {
        if self.stack.is_empty() {
            None
        } else {
            let len = self.stack.len();
            Some(&mut self.stack[len - 1])
        }
    }

    pub fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

pub struct Translator<'tcx> {
    tcx: TyCtxt<'tcx>,
    call_stack: CallStack<(Function<'tcx>)>,
    pnml_doc: PNMLDocument,
    net_ref: PetriNetRef,
    root_page: PageRef,
}

macro_rules! net {
    ($translator:ident) => {
        $translator
            .pnml_doc
            .petri_net_data($translator.net_ref)
            .expect("corrupted net reference")
    };
}

macro_rules! function {
    ($translator:ident) => {
        $translator.call_stack.peek_mut().expect("empty call stack")
    };
}

impl<'tcx> Translator<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>) -> Result<Self> {
        let mut pnml_doc = PNMLDocument::new();
        //TODO: find a descriptive name e.g. name of the program
        let net_ref = pnml_doc.add_petri_net(None);
        let root_page = pnml_doc
            .petri_net_data(net_ref)
            .expect("corrupted net reference")
            .add_page(Some("entry"));
        Ok(Translator {
            tcx,
            call_stack: CallStack::new(),
            pnml_doc,
            net_ref,
            root_page,
        })
    }

    pub fn petrify(&mut self, main_fn: DefId) -> Result<()> {
        let start_place = {
            let net = net!(self);
            let mut place = net.add_place(&self.root_page)?;
            place.initial_marking(net, 1)?;
            place
        };
        //TODO: check destination
        self.translate(main_fn, start_place, &Vec::new(), &None)?;
        print!("{}", self.pnml_doc.to_xml()?);
        Ok(())
    }

    fn translate<'a>(
        &mut self,
        function: DefId,
        start_place: NodeRef,
        args: &Vec<Operand<'_>>,
        destination: &Option<(Place<'tcx>, mir::BasicBlock)>,
    ) -> Result<()> {
        let fn_name = function.describe_as_module(self.tcx);
        info!("ENTERING function: {:?}", fn_name);
        let body = self.tcx.optimized_mir(function);
        // if we come from the main we ignore the arguments
        // else we pass the locals for the function arguments
        let args = if args.is_empty() {
            std::collections::HashMap::new()
        } else {
            let mut map = std::collections::HashMap::new();
            for arg in args {
                let local = op_to_local(arg);
                map.insert(local.clone(), function!(self).get_local(local)?.clone());
            }
            map
        };
        // if we got a none we stepped into a converging function
        // if we come from the main we create a local for a return
        // else we get the return place from the caller
        let destination = {
            match destination {
                None => None,
                Some((place, _block)) => {
                    let local = place_to_local(place);
                    if self.call_stack.is_empty() {
                        let mut place = net!(self).add_place(&self.root_page)?;
                        Some((
                            local.clone(),
                            crate::petri_net::function::Local::new(net!(self), &self.root_page)?,
                        ))
                    } else {
                        Some((local.clone(), function!(self).get_local(local)?.clone()))
                    }
                }
            }
        };
        let petri_function = Function::new(
            function,
            body,
            net!(self),
            args,
            destination,
            start_place,
            &fn_name,
        )?;
        self.call_stack.push(petri_function);
        self.visit_body(body);
        self.call_stack.pop();
        info!("LEAVING function: {:?}", fn_name);
        Ok(())
    }
}

impl<'tcx> Visitor<'tcx> for Translator<'tcx> {
    fn visit_body(&mut self, body: &Body<'tcx>) {
        match body.phase {
            MirPhase::Optimized => {
                trace!("source scopes: {:?}", body.source_scopes);
                trace!(
                    "source scopes local data: {:?}",
                    body.source_scope_local_data
                );
                //trace!("promoted: {:?}", entry_body.promoted);
                trace!("return type: {:?}", body.return_ty());
                trace!("yield type: {:?}", body.yield_ty);
                trace!("generator drop: {:?}", body.generator_drop);
                //trace!("local declarations: {:?}", body.local_decls());
            }
            _ => error!("tried to translate unoptimized MIR"),
        }
        function!(self)
            .add_locals(net!(self), &body.local_decls)
            .expect("cannot add locals to petri net function");
        self.super_body(body);
    }

    fn visit_basic_block_data(&mut self, block: BasicBlock, data: &BasicBlockData<'tcx>) {
        trace!("\n---BasicBlock {:?}---", block);
        function!(self)
            .activate_block(net!(self), &block)
            .expect("unable to activate basic");
        self.super_basic_block_data(block, data)
    }
    fn visit_source_scope_data(&mut self, scope_data: &SourceScopeData) {
        self.super_source_scope_data(scope_data);
    }

    fn visit_statement(&mut self, statement: &Statement<'tcx>, location: Location) {
        trace!("{:?}: ", statement.kind);
        function!(self)
            .add_statement(net!(self))
            .expect("unable to add statement");
        self.super_statement(statement, location);
    }

    //Begin statement visits//
    fn visit_assign(&mut self, place: &Place<'tcx>, rvalue: &Rvalue<'tcx>, location: Location) {
        //trace!("{:?} = {:?}", place, rvalue);
        panic!("assign"); // TODO: remove
        self.super_assign(place, rvalue, location);
    }

    fn visit_place(&mut self, place: &Place<'tcx>, context: PlaceContext, location: Location) {
        panic!("place"); // TODO: remove
        self.super_place(place, context, location);
    }

    fn visit_local(&mut self, _local: &Local, _context: PlaceContext, _location: Location) {
        trace!("local");
    }

    fn visit_retag(&mut self, kind: &RetagKind, place: &Place<'tcx>, location: Location) {
        trace!("{:?}@{:?}", kind, place);
        panic!("retag"); //TODO: remove
        self.super_retag(kind, place, location);
    }

    fn visit_ascribe_user_ty(
        &mut self,
        place: &Place<'tcx>,
        variance: &ty::Variance,
        user_ty: &UserTypeProjection,
        location: Location,
    ) {
        panic!("ascribe_user_ty"); //TODO: remove
        self.super_ascribe_user_ty(place, variance, user_ty, location);
    }
    //End statement visits

    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        //trace!("{:?}", terminator);
        //warn!("Successors: {:?}", terminator.successors());
        self.super_terminator(terminator, location);
    }

    fn visit_terminator_kind(&mut self, kind: &TerminatorKind<'tcx>, location: Location) {
        trace!("{:?}", kind);
        use rustc::mir::TerminatorKind::*;
        let net = net!(self);
        match kind {
            Return => {
                trace!("Return");
                function!(self).retorn(net).expect("return failed");
            }

            Goto { target } => {
                trace!("Goto");
                function!(self)
                    .goto(net, target)
                    .expect("Goto Block failed");
            }

            SwitchInt { .. } => panic!("SwitchInt"),

            Call {
                ref func,
                args,
                destination,
                ..
            } => {
                // info!(
                //     "functionCall\nfunc: {:?}\nargs: {:?}\ndest: {:?}",
                //     func, args, destination
                // );
                let sty = {
                    match func {
                        Operand::Copy(ref place) | Operand::Move(ref place) => {
                            let function = self.call_stack.peek().expect("peeked empty stack");
                            let decls = function.mir_body.local_decls();
                            let place_ty: &mir::tcx::PlaceTy<'tcx> = &place.base.ty(decls);
                            place_ty.ty
                        }
                        Operand::Constant(ref constant) => &constant.ty,
                    }
                };
                let function = match sty.sty {
                    ty::FnPtr(_) => {
                        error!("Function pointers are not supported");
                        panic!("")
                    }
                    ty::FnDef(def_id, _) => def_id,
                    _ => {
                        error!("Expected function definition or pointer but got: {:?}", sty);
                        panic!("")
                    }
                };
                if self.tcx.is_foreign_item(function) {
                    warn!("found foreign item: {:?}", function);
                } else {
                    if !skip_function(self.tcx, function) {
                        if !self.tcx.is_mir_available(function) {
                            warn!("Could not find mir: {:?}", function);
                        } else {
                            let start_place = function!(self)
                                .function_call_start_place()
                                .expect("Unable to infer start place of function call")
                                .clone();
                            self.translate(function, start_place, args, destination);
                        }
                    }
                }
            }

            Drop { .. } => {
                panic! {"drop"}
            }

            Assert { .. } => warn!("assert"),

            Yield { .. } => warn!("Yield"),
            GeneratorDrop => warn!("GeneratorDrop"),
            DropAndReplace { .. } => warn!("DropAndReplace"),
            Resume => warn!("Resume"),
            Abort => warn!("Abort"),
            FalseEdges { .. } => bug!(
                "should have been eliminated by\
                 `simplify_branches` mir pass"
            ),
            FalseUnwind { .. } => bug!(
                "should have been eliminated by\
                 `simplify_branches` mir pass"
            ),
            Unreachable => error!("unreachable"),
        }
        self.super_terminator_kind(kind, location);
    }

    fn visit_assert_message(&mut self, msg: &AssertMessage<'tcx>, location: Location) {
        self.super_assert_message(msg, location);
    }

    fn visit_rvalue(&mut self, rvalue: &Rvalue<'tcx>, location: Location) {
        self.super_rvalue(rvalue, location);
    }

    fn visit_operand(&mut self, operand: &Operand<'tcx>, location: Location) {
        self.super_operand(operand, location);
    }

    fn visit_place_base(
        &mut self,
        place_base: &PlaceBase<'tcx>,
        context: PlaceContext,
        location: Location,
    ) {
        self.super_place_base(place_base, context, location);
    }

    // fn visit_projection(&mut self,
    //                     place: &Projection<'tcx>,
    //                     context: PlaceContext,
    //                     location: Location) {
    //     self.super_projection(place, context, location);
    // }

    fn visit_constant(&mut self, constant: &Constant<'tcx>, location: Location) {
        trace!("Constant: {:?}", constant);
        self.super_constant(constant, location);
    }

    // fn visit_span(&mut self,
    //               span: &Span) {
    //     self.super_span(span);
    // }

    fn visit_source_info(&mut self, source_info: &SourceInfo) {
        self.super_source_info(source_info);
    }

    fn visit_ty(&mut self, ty: Ty<'tcx>, _: TyContext) {
        self.super_ty(ty);
    }

    fn visit_user_type_projection(&mut self, ty: &UserTypeProjection) {
        self.super_user_type_projection(ty);
    }

    // fn visit_user_type_annotation(
    //     &mut self,
    //     index: UserTypeAnnotationIndex,
    //     ty: &CanonicalUserTypeAnnotation<'tcx>,
    // ) {
    //     self.super_user_type_annotation(index, ty);
    // }

    fn visit_region(&mut self, region: &ty::Region<'tcx>, _: Location) {
        self.super_region(region);
    }

    fn visit_const(&mut self, constant: &&'tcx ty::Const<'tcx>, _: Location) {
        trace!("Const: {:?}", constant);
        self.super_const(constant);
    }

    // fn visit_substs(&mut self,
    //                 substs: &SubstsRef<'tcx>,
    //                 _: Location) {
    //     self.super_substs(substs);
    // }

    fn visit_closure_substs(&mut self, substs: &ClosureSubsts<'tcx>, _: Location) {
        self.super_closure_substs(substs);
    }

    fn visit_generator_substs(&mut self, substs: &GeneratorSubsts<'tcx>, _: Location) {
        self.super_generator_substs(substs);
    }

    fn visit_local_decl(&mut self, local: Local, local_decl: &LocalDecl<'tcx>) {
        self.super_local_decl(local, local_decl);
    }

    fn visit_source_scope(&mut self, scope: &SourceScope) {
        self.super_source_scope(scope);
    }
}

fn skip_function<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> bool {
    //FIXME: check if a call for a panic always result in a panic (it might be caught later)
    if tcx.lang_items().items().contains(&Some(def_id)) {
        debug!("LangItem: {:?}", def_id);
    };
    if Some(def_id) == tcx.lang_items().panic_fn() {
        trace!("panic");
        return true;
    }
    let description = def_id.describe_as_module(tcx);
    if description.contains("std::rt::begin_panic_fmt") {
        true
    } else if description.contains("std::panicking::panicking") {
        true
    } else {
        false
    }
}

fn op_to_local<'a>(operand: &'a Operand<'a>) -> &'a Local {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => place_to_local(place),
        Operand::Constant(_) => panic!("cannot convert Constant to Local"),
    }
}

fn place_to_local<'a>(place: &'a Place<'a>) -> &'a Local {
    match &place.base {
        PlaceBase::Local(local) => local,
        PlaceBase::Static(_statik) => panic!("static places cannot (yet) be converted to locals"),
    }
}
