use std::cell::RefCell;
use std::collections::HashSet;
use std::ops::DerefMut;

use rustc_middle::ty::{
    AliasTy, Const, ConstKind, ExistentialPredicate, ExistentialProjection, ExistentialTraitRef,
    FnSig, ParamConst, ParamTy, Ty, TyCtxt, TyKind,
};
use rustc_middle::ty::{GenericArg, GenericArgKind, GenericArgsRef};
use rustc_span::def_id::DefId;

use crate::utils;

pub struct Monomorphizer<'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub generic_args: Vec<GenericArg<'tcx>>,
    pub closures_being_monomorphized: RefCell<HashSet<DefId>>,
}

impl<'tcx> Monomorphizer<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, generic_args: Vec<GenericArg<'tcx>>) -> Monomorphizer<'tcx> {
        Monomorphizer {
            tcx,
            generic_args,
            closures_being_monomorphized: RefCell::new(HashSet::new()),
        }
    }

    pub fn mono_arguments(&self, args: GenericArgsRef<'tcx>) -> GenericArgsRef<'tcx> {
        let monoed_args: Vec<GenericArg<'_>> = args
            .iter()
            .map(|gen_arg| self.mono_argument(gen_arg))
            .collect();
        self.tcx.mk_args(&monoed_args)
    }

    fn mono_argument(&self, generic_argument: GenericArg<'tcx>) -> GenericArg<'tcx> {
        match generic_argument.unpack() {
            GenericArgKind::Type(ty) => self.mono_type(ty).into(),
            GenericArgKind::Const(c) => self.mono_const(c).into(),
            GenericArgKind::Lifetime(_) => generic_argument,
        }
    }

    fn mono_const(&self, constant: Const<'tcx>) -> Const<'tcx> {
        if let ConstKind::Param(ParamConst { index, name: _ }) = constant.kind() {
            match self.generic_args[index as usize].unpack() {
                GenericArgKind::Const(c) => c,
                _ => {
                    tracing::debug!(
                        "Err: {:?}({:?})",
                        self.generic_args[index as usize],
                        constant.kind()
                    );
                    constant
                }
            }
        } else {
            constant
        }
    }

    pub fn mono_type(&self, generic_type: Ty<'tcx>) -> Ty<'tcx> {
        // Handle associated type projections (e.g., `<T as Trait<..>>::N`)
        // This is where we resolve a type alias to its concrete type

        if let TyKind::Alias(rustc_middle::ty::Projection, projection) = generic_type.kind() {
            return self.mono_alias_projection(generic_type, *projection);
        }

        // If the type is an opaque type, substitute it with the concrete type.
        // An opaque type is usually from impl Trait in type aliases or function return types
        if let TyKind::Alias(
            rustc_middle::ty::Opaque,
            rustc_middle::ty::AliasTy { def_id, args, .. },
        ) = generic_type.kind()
        {
            let gen_args = self.mono_arguments(args).to_vec();
            let underlying_type = self.tcx.type_of(def_id).skip_binder();
            let monomorphized_type =
                Monomorphizer::new(self.tcx, gen_args).mono_type(underlying_type);
            // debug!("Opaque type {:?} monomorphized to {:?}", gen_arg_type, monomorphized_type);
            return monomorphized_type;
        }

        match generic_type.kind() {
            TyKind::Adt(def, args) => Ty::new_adt(self.tcx, *def, self.mono_arguments(args)),
            TyKind::Array(elem_ty, len) => {
                let monomorphized_elem_ty = self.mono_type(*elem_ty);
                let monomorphized_len = self.mono_const(*len);
                self.tcx
                    .mk_ty_from_kind(TyKind::Array(monomorphized_elem_ty, monomorphized_len))
            }
            TyKind::Slice(elem_ty) => {
                let monomorphized_elem_ty = self.mono_type(*elem_ty);
                Ty::new_slice(self.tcx, monomorphized_elem_ty)
            }
            TyKind::RawPtr(ty, mutbl) => {
                let monomorphized_ty = self.mono_type(*ty);
                Ty::new_ptr(self.tcx, monomorphized_ty, *mutbl)
            }
            TyKind::Ref(region, ty, mutbl) => {
                let monomorphized_ty = self.mono_type(*ty);
                Ty::new_ref(self.tcx, *region, monomorphized_ty, *mutbl)
            }
            TyKind::FnDef(def_id, substs) => {
                Ty::new_fn_def(self.tcx, *def_id, self.mono_arguments(substs))
            }
            TyKind::FnPtr(fn_sig) => {
                let map_fn_sig = |fn_sig: FnSig<'tcx>| {
                    let monomorphized_inputs_and_output = self.tcx.mk_type_list_from_iter(
                        fn_sig.inputs_and_output.iter().map(|ty| self.mono_type(ty)),
                    );
                    FnSig {
                        inputs_and_output: monomorphized_inputs_and_output,
                        c_variadic: fn_sig.c_variadic,
                        safety: fn_sig.safety,
                        abi: fn_sig.abi,
                    }
                };
                let monomorphized_fn_sig = fn_sig.map_bound(map_fn_sig);
                Ty::new_fn_ptr(self.tcx, monomorphized_fn_sig)
            }
            TyKind::Dynamic(predicates, region, kind) => {
                let monomorphized_predicates = predicates.iter().map(
                    |bound_pred: rustc_middle::ty::Binder<'_, ExistentialPredicate<'tcx>>| {
                        bound_pred.map_bound(|pred| match pred {
                            ExistentialPredicate::Trait(ExistentialTraitRef { def_id, args }) => {
                                ExistentialPredicate::Trait(ExistentialTraitRef {
                                    def_id,
                                    args: self.mono_arguments(args),
                                })
                            }
                            ExistentialPredicate::Projection(ExistentialProjection {
                                def_id,
                                args,
                                term,
                            }) => {
                                if let Some(ty) = term.as_type() {
                                    ExistentialPredicate::Projection(ExistentialProjection {
                                        def_id,
                                        args: self.mono_arguments(args),
                                        term: self.mono_type(ty).into(),
                                    })
                                } else {
                                    ExistentialPredicate::Projection(ExistentialProjection {
                                        def_id,
                                        args: self.mono_arguments(args),
                                        term,
                                    })
                                }
                            }
                            ExistentialPredicate::AutoTrait(_) => pred,
                        })
                    },
                );
                Ty::new_dynamic(
                    self.tcx,
                    self.tcx
                        .mk_poly_existential_predicates_from_iter(monomorphized_predicates),
                    *region,
                    *kind,
                )
            }
            TyKind::Closure(def_id, args) => {
                // Closure types can be part of their own type parameters...
                // so need to guard against endless recursion
                {
                    let mut borrowed_closures_being_monomorphized =
                        self.closures_being_monomorphized.borrow_mut();
                    let closures_being_monomorphized =
                        borrowed_closures_being_monomorphized.deref_mut();
                    if !closures_being_monomorphized.insert(*def_id) {
                        return generic_type;
                    }
                }
                let monomorphized_closure =
                    Ty::new_closure(self.tcx, *def_id, self.mono_arguments(args));
                let mut borrowed_closures_being_monomorphized =
                    self.closures_being_monomorphized.borrow_mut();
                let closures_being_monomorphized =
                    borrowed_closures_being_monomorphized.deref_mut();
                closures_being_monomorphized.remove(def_id);
                monomorphized_closure
            }
            TyKind::Coroutine(def_id, args) => {
                Ty::new_coroutine(self.tcx, *def_id, self.mono_arguments(args))
            }
            TyKind::CoroutineWitness(_def_id, _args) => {
                // Todo: monomorphize generic arguments for a CoroutineWitness type
                generic_type
            }
            TyKind::Tuple(types) => {
                Ty::new_tup_from_iter(self.tcx, types.iter().map(|ty| self.mono_type(ty)))
            }
            TyKind::Param(ParamTy { index, name: _ }) => {
                match self.generic_args[*index as usize].unpack() {
                    GenericArgKind::Type(ty) => ty,
                    _ => {
                        tracing::error!(
                            "Unexpected param type: {:?}({:?})",
                            self.generic_args[*index as usize],
                            generic_type.kind()
                        );
                        unreachable!();
                    }
                }
            }
            _ => generic_type,
        }
    }

    fn mono_alias_projection(&self, generic_type: Ty<'tcx>, projection: AliasTy<'tcx>) -> Ty<'tcx> {
        // 第一步，单态化投影里的参数
        let monoed_substs = self.mono_arguments(projection.args);

        if utils::are_concrete(monoed_substs) {
            self.mono_alias_projection_with_args_concrete(generic_type, projection, monoed_substs)
        } else {
            Ty::new_projection(self.tcx, projection.def_id, monoed_substs)
        }
    }

    fn mono_alias_projection_with_args_concrete(
        &self,
        generic_type: Ty<'tcx>,
        projection: AliasTy,
        monoed_substs: GenericArgsRef<'tcx>,
    ) -> Ty<'tcx> {
        // 如果单态化后的投影参数都是concrete类型
        // 用关联类型来得到param_env

        // 获得投影的ID，后面要获得关联的item，如trait Trait<..>{ type N; }里面的N
        let item_def_id = projection.def_id;
        // 获得contrainer的id，即所在trait的def_id
        let container_def_id = self.tcx.associated_item(item_def_id).container_id(self.tcx);
        // 获得trait的参数环境
        let param_env = self.tcx.param_env(container_def_id);

        // 先尝试对投影进行归一化
        {
            // 创建单态化后的projection
            let new_proj_type = Ty::new_projection(self.tcx, item_def_id, monoed_substs);

            // 先尝试归一化，三个参数
            // 1. trait的参数环境 2. 关联变量item的id 3. 归一化的substs
            if let Ok(monoed_type) = self
                .tcx
                .try_normalize_erasing_regions(param_env, new_proj_type)
            {
                return monoed_type;
            }
        }

        // 如果归一化不行，就进行解析resolve，三个参数
        // 1. trait的参数环境 2. 关联变量item的id 3. 归一化的substs
        if let Ok(Some(instance)) =
            rustc_middle::ty::Instance::try_resolve(self.tcx, param_env, item_def_id, monoed_substs)
        {
            let instance_item_def_id = instance.def.def_id();
            if item_def_id == instance_item_def_id {
                // Resolve the concrete type for FnOnce::Output alias type.
                // It may omit to resolve a closure's output type, in which case
                // the resolved instance_item_def_id may correspond to FnOnce::call_once
                // instead of FnOnce::Output, leading to item_def_id not equal to instance_item_def_id.
                if utils::is_fn_once_output(self.tcx, instance_item_def_id) {
                    if monoed_substs.len() > 0 {
                        if let Some(ty) = monoed_substs[0].as_type() {
                            match ty.kind() {
                                TyKind::FnDef(def_id, gen_args) => {
                                    return utils::function_return_type(
                                        self.tcx, *def_id, gen_args,
                                    );
                                }
                                TyKind::Closure(def_id, gen_args) => {
                                    return utils::closure_return_type(self.tcx, *def_id, gen_args);
                                }
                                TyKind::FnPtr(fn_sig) => {
                                    return fn_sig.skip_binder().output();
                                }
                                _ => {}
                            }
                        }
                    }
                }
                return Ty::new_projection(self.tcx, projection.def_id, monoed_substs);
            }
            let item_type = self.tcx.type_of(instance_item_def_id).skip_binder();
            if utils::is_fn_once_output(self.tcx, item_def_id)
                && utils::is_fn_once_call_once(self.tcx, instance_item_def_id)
            {
                if monoed_substs.len() > 0 {
                    if let Some(ty) = monoed_substs[0].as_type() {
                        if let TyKind::Closure(def_id, gen_args) = ty.kind() {
                            let specialized_type =
                                utils::closure_return_type(self.tcx, *def_id, gen_args);
                            tracing::debug!(
                                "FnOnce::Output ({:?}) specialized to {:?}",
                                ty,
                                specialized_type
                            );
                            return specialized_type;
                        }
                    }
                }
            }
            let tmp_generic_args = instance.args.to_vec();
            let tmp_specializer = Monomorphizer::new(self.tcx, tmp_generic_args);
            tmp_specializer.mono_type(item_type)
        } else {
            let projection_trait = Some(self.tcx.parent(item_def_id));
            if projection_trait == self.tcx.lang_items().pointee_trait() {
                assert!(!monoed_substs.is_empty());
                if let GenericArgKind::Type(ty) = monoed_substs[0].unpack() {
                    return ty.ptr_metadata_ty(self.tcx, |ty| ty);
                }
            } else if projection_trait == self.tcx.lang_items().discriminant_kind_trait() {
                assert!(!monoed_substs.is_empty());
                if let GenericArgKind::Type(enum_ty) = monoed_substs[0].unpack() {
                    return enum_ty.discriminant_ty(self.tcx);
                }
            }
            tracing::warn!("Could not resolve an associated type with concrete type arguments");
            generic_type
        }
    }
}
