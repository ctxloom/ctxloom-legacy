# Messages for the crate's errors. Each id is a code, which begins with the
# crate's name; the variables are its args. An id ending -fix is the remedy of the one before.

library-git-git-missing = git is not on the PATH, and ctxloom reads its library through git
library-git-git-missing-fix = install git (2.25 or newer) and put it on the PATH, then run the command again
library-git-git-too-old = git { $version } is older than 2.25, the oldest ctxloom reads its library with
library-git-git-too-old-fix = install git 2.25 or newer
library-git-failed = git { $args } failed in { $dir }: { $ext_err_txt }
