use std::collections::HashMap;
use std::any::Any;
use std::collections::hash_map::Drain;

pub type ProgressVar = Box<dyn Any + Send>;
#[derive(Debug, Default)]
pub struct ProgressVars {
    vars: HashMap<String, ProgressVar>,
}

#[macro_export]
macro_rules! delegate_progressvars_on {
    ($name:tt) => {
        delegate ! {
            to self.$name {
                pub fn extract_var<S: AsRef<str>>(&mut self, key: S) -> Option<Box<dyn Any + Send>>;
                pub fn insert_var<S: AsRef<str>>(&mut self, key: S, boxed: Box<dyn Any + Send>);
                pub fn var_exists<S: AsRef<str>>(&self, key: S) -> bool;
                pub fn drain_vars(&mut self) -> Drain<'_, String, Box<(dyn Any + Send)>>;
                pub fn clone_var<T: Clone + 'static>(&self, key: &str) -> Option<T>;
                pub fn peak_var<F, T: 'static>(&mut self, key: &str, ok_cb: F)
                    where F: FnMut(&T);
            }
        }
    };
}

impl ProgressVars {
    /// this returns the Box<dyn Any> that
    /// is referenced by the key. if you want this
    /// variable to be reused later, you must
    /// reinsert it by calling insert_var
    pub fn extract_var<S: AsRef<str>>(
        &mut self,
        key: S
    ) -> Option<Box<dyn Any + Send>>{
        self.vars.remove(key.as_ref())
    }

    /// creates a string from your key to be inserted
    /// into the vars hashmap. no check is done to see
    /// if the variable exists prior to inserting, so
    /// that is up to you to do by checking var_exists if desired
    pub fn insert_var<S: AsRef<str>>(&mut self, key: S, boxed: Box<dyn Any + Send>) {
        self.vars.insert(key.as_ref().to_string(), boxed);
    }

    pub fn var_exists<S: AsRef<str>>(&self, key: S) -> bool {
        self.vars.contains_key(key.as_ref())
    }

    pub fn drain_vars(&mut self) -> Drain<'_, String, Box<(dyn Any + Send)>> {
        self.vars.drain()
    }

    pub fn peak_var<F, T: 'static>(
        &self,
        key: &str,
        ok_cb: F,
    )
        where F: FnMut(&T)
    {
        let mut mut_ok_cb = ok_cb;
        if let Some(boxed) = self.vars.get(key) {
            let boxed_ref = boxed.as_ref();
            if let Some(var_value) = boxed_ref.downcast_ref::<T>() {
                mut_ok_cb(var_value);
            }
        }
    }

    pub fn clone_var<T: Clone + 'static>(
        &self,
        key: &str,
    ) -> Option<T> {
        if let Some(boxed) = self.vars.get(key) {
            let boxed_ref = boxed.as_ref();
            if let Some(var_value) = boxed_ref.downcast_ref::<T>() {
                return Some(var_value.clone());
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_use_progress_vars() {
        let mut progvars = ProgressVars::default();
        let key = "key";
        progvars.insert_var(
            String::from(key),
            Box::new(String::from("some value"))
        );
        let boxed = progvars.extract_var(key).unwrap();
        if let Ok(x) = boxed.downcast::<String>() {
            assert_eq!(x.as_str(), "some value");
        } else {
            assert!(false);
        }
    }

    #[test]
    fn can_peak_var() {
        let mut progvars = ProgressVars::default();
        let key = "key";
        progvars.insert_var(
            String::from(key),
            Box::new(String::from("aaaaa"))
        );
        let mut cb_called = false;
        progvars.peak_var::<_, String>("key", |mystring| {
            assert_eq!(mystring, "aaaaa");
            cb_called = true;
        });
        assert!(cb_called)
    }

    #[test]
    fn can_clone_var() {
        let mut progvars = ProgressVars::default();
        let key = "key";
        progvars.insert_var(
            String::from(key),
            Box::new(String::from("aaaaa"))
        );
        let mystring = progvars.clone_var::<String>("key").unwrap();
        assert_eq!(mystring, "aaaaa");
    }
}
