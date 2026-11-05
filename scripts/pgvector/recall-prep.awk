# Turns GloVe text ("<word> <dim values>" per line) into the recall tier's rows
# (M7). recall-data.sh runs it with LC_ALL=C and -v rows, queries, dim, nnz,
# base and query, the last two being the output files. Each output line is
#   <line number> TAB [<dense values>] TAB {<i:x,...>}/<dim>
# with values copied verbatim from the text and i 1-based. The sparse form
# keeps the nnz largest magnitudes, ties to the lower index, in index order.
# Lines 1 to rows + queries are used; a line is a query when its number
# divides by the stride, which makes exactly `queries` of them.

BEGIN {
  stride = rows / queries + 1
  need = rows + queries
}

# Read to the end of the stream, ignoring what is not needed, rather than exit
# here: an early exit would kill unzip with SIGPIPE, so pipefail could not tell
# that from a real unzip failure. The fields stay unsplit, so this is cheap.
NR > need { next }

NF != dim + 1 {
  printf "recall-prep: line %d has %d fields, expected %d\n", NR, NF, dim + 1 > "/dev/stderr"
  exit 1
}

{
  # top[1..n], mag[1..n]: the largest magnitudes so far, largest first. Indexes
  # arrive in ascending order and only a strictly larger magnitude moves ahead
  # of an equal one, so ties stay with the lower index.
  n = 0
  for (i = 1; i <= dim; i++) {
    m = $(i + 1) + 0
    if (m < 0) m = -m
    if (n == nnz && m <= mag[n]) continue
    if (n < nnz) n++
    for (j = n; j > 1 && m > mag[j - 1]; j--) {
      mag[j] = mag[j - 1]
      top[j] = top[j - 1]
    }
    mag[j] = m
    top[j] = i
  }
  for (j = 1; j <= n; j++) kept[top[j]] = NR

  dense = sparse = ""
  for (i = 1; i <= dim; i++) {
    dense = dense (i > 1 ? "," : "") $(i + 1)
    if (kept[i] == NR) sparse = sparse (sparse == "" ? "" : ",") i ":" $(i + 1)
  }
  line = NR "\t[" dense "]\t{" sparse "}/" dim
  if (NR % stride == 0) print line > query
  else print line > base
}
