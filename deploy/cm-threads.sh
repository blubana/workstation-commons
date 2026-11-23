# /etc/profile.d/cm-threads.sh — default thread pools for shared use (compute-mgr).
# 24 BLAS threads per process oversubscribes the CPU when several users compute at once
# (measured: 4 jobs x 24 threads 23.2 s vs x 6 threads 11.7 s; single job 24 thr 3.0 s vs 12 thr 1.7 s).
# Override per session:  export CM_THREADS=24   (or set OMP/OPENBLAS/MKL_NUM_THREADS yourself).
# /etc/environment also sets 12 (covers VS Code / PyCharm / Zed sessions); CM_THREADS wins over both.
_cm_t=${CM_THREADS:-12}
for _v in OMP_NUM_THREADS OPENBLAS_NUM_THREADS MKL_NUM_THREADS NUMEXPR_MAX_THREADS NUMBA_NUM_THREADS; do
  if [ -n "${CM_THREADS:-}" ]; then export $_v=$_cm_t; else eval "[ -n \"\${$_v}\" ] || export $_v=$_cm_t"; fi
done
unset _cm_t _v
