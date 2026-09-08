def left():
    return 1


def right():
    return 2


def dispatch(flag):
    choice = left if flag else right
    return choice()


dispatch(True)
