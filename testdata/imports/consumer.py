import pkg.deep.module as aliased_module
from pkg import deep
from pkg.deep import module
from pkg.deep.module import target as aliased_target
from pkg import target
import other

aliased_module.target()
deep.module.target()
module.target()
aliased_target()
target()
other.target()

